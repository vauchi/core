#!/usr/bin/env python3
"""
Refactoring script to move inline #[cfg(test)] modules from src/ to tests/ directories.

This script follows the CLAUDE.md rule: src/ = production only, tests/ = tests only.

Usage:
    python3 scripts/refactor_tests.py analyze              # Show what would change
    python3 scripts/refactor_tests.py extract              # Extract tests (creates backup)
    python3 scripts/refactor_tests.py verify               # Run cargo test to verify
    python3 scripts/refactor_tests.py rollback             # Restore from backup
    python3 scripts/refactor_tests.py --crate core extract # Single crate

Strategy:
    For Rust, inline tests use `super::*` to access private items. When moving to
    external test files, these become integration tests that can only access pub items.

    This script:
    1. Backs up all files before modifying
    2. Extracts test modules to tests/ directory
    3. Removes inline test modules from src/ files
    4. Provides rollback capability
"""

import os
import re
import argparse
import subprocess
import shutil
import json
from pathlib import Path
from dataclasses import dataclass, field
from datetime import datetime
from typing import Optional


BACKUP_DIR = Path('.test_refactor_backup')


@dataclass
class InlineTestModule:
    """Represents an inline #[cfg(test)] mod tests block."""
    file_path: Path
    module_name: str
    start_line: int
    end_line: int
    content: str
    test_functions: list = field(default_factory=list)


def find_test_module_bounds(lines: list[str], start_idx: int) -> tuple[int, str]:
    """Find the end of a #[cfg(test)] mod tests { ... } block."""
    brace_count = 0
    content_lines = []
    in_block = False

    for i in range(start_idx, len(lines)):
        line = lines[i]
        content_lines.append(line)

        for char in line:
            if char == '{':
                brace_count += 1
                in_block = True
            elif char == '}':
                brace_count -= 1

        if in_block and brace_count == 0:
            return i, ''.join(content_lines)

    return len(lines) - 1, ''.join(content_lines)


def extract_test_functions(content: str) -> list[str]:
    """Extract individual test function names from a test module."""
    pattern = r'#\[test\]\s*(?:#\[.*?\]\s*)*fn\s+(\w+)'
    return re.findall(pattern, content)


def find_inline_tests(src_dir: Path) -> list[InlineTestModule]:
    """Find all inline #[cfg(test)] modules in a src directory."""
    results = []

    for rs_file in src_dir.rglob("*.rs"):
        if not rs_file.is_file():
            continue

        content = rs_file.read_text()
        lines = content.split('\n')

        i = 0
        while i < len(lines):
            line = lines[i]

            if re.match(r'\s*#\[cfg\(test\)\]\s*$', line):
                j = i + 1
                while j < len(lines) and lines[j].strip() == '':
                    j += 1

                if j < len(lines) and re.match(r'\s*mod\s+tests\s*\{?', lines[j]):
                    end_idx, module_content = find_test_module_bounds(lines, i)

                    rel_path = rs_file.relative_to(src_dir)
                    parts = list(rel_path.with_suffix('').parts)
                    if parts[-1] == 'mod':
                        parts = parts[:-1]
                    module_name = '::'.join(parts) if parts else 'lib'

                    results.append(InlineTestModule(
                        file_path=rs_file,
                        module_name=module_name,
                        start_line=i + 1,
                        end_line=end_idx + 1,
                        content=module_content,
                        test_functions=extract_test_functions(module_content),
                    ))
                    i = end_idx
            i += 1

    return results


def extract_test_body(content: str) -> str:
    """Extract the inner content of a test module."""
    lines = content.split('\n')
    inner_lines = []
    brace_count = 0
    started = False

    for line in lines:
        if not started:
            if '{' in line:
                idx = line.index('{')
                rest = line[idx + 1:]
                if rest.strip():
                    inner_lines.append(rest)
                started = True
                brace_count = 1
                for c in rest:
                    if c == '{':
                        brace_count += 1
                    elif c == '}':
                        brace_count -= 1
        else:
            for c in line:
                if c == '{':
                    brace_count += 1
                elif c == '}':
                    brace_count -= 1

            if brace_count > 0:
                inner_lines.append(line)
            elif brace_count == 0:
                stripped = line.rstrip()
                if stripped.endswith('}'):
                    stripped = stripped[:-1]
                if stripped.strip():
                    inner_lines.append(stripped)
                break

    return '\n'.join(inner_lines)


def generate_integration_test(crate_name: str, module_name: str, module: InlineTestModule) -> str:
    """Generate an integration test file from inline tests."""
    crate_mod = crate_name.replace('-', '_')
    inner_content = extract_test_body(module.content)

    # Transform super:: references to crate imports
    if module_name and module_name != 'lib':
        import_path = f"{crate_mod}::{module_name}"
    else:
        import_path = crate_mod

    inner_content = re.sub(r'use super::\*;', f'use {import_path}::*;', inner_content)
    inner_content = re.sub(r'super::(\w+)', f'{import_path}::\\1', inner_content)

    # Remove leading indentation (usually 4 spaces from mod tests {})
    lines = inner_content.split('\n')
    min_indent = float('inf')
    for line in lines:
        if line.strip():
            indent = len(line) - len(line.lstrip())
            min_indent = min(min_indent, indent)

    if min_indent != float('inf') and min_indent > 0:
        lines = [line[min_indent:] if len(line) >= min_indent else line for line in lines]
        inner_content = '\n'.join(lines)

    header = f'''//! Tests for {module_name}
//! Extracted from {module.file_path.name}

'''
    return header + inner_content.strip() + '\n'


def remove_inline_tests_from_source(file_path: Path, modules: list[InlineTestModule]) -> str:
    """Remove inline test modules from a source file."""
    content = file_path.read_text()
    lines = content.split('\n')

    sorted_modules = sorted(modules, key=lambda m: m.start_line, reverse=True)

    for module in sorted_modules:
        start_idx = module.start_line - 1
        end_idx = module.end_line

        # Remove preceding blank lines
        while start_idx > 0 and lines[start_idx - 1].strip() == '':
            start_idx -= 1
            if start_idx > 0 and lines[start_idx - 1].strip() != '':
                start_idx += 1
                break

        del lines[start_idx:end_idx]

    # Clean up trailing whitespace
    while lines and lines[-1].strip() == '':
        lines.pop()
    lines.append('')

    return '\n'.join(lines)


def backup_files(root: Path, files: list[Path]) -> Path:
    """Create a backup of files before modification."""
    timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
    backup_path = root / BACKUP_DIR / timestamp

    backup_path.mkdir(parents=True, exist_ok=True)

    manifest = {'timestamp': timestamp, 'files': []}

    for file_path in files:
        rel_path = file_path.relative_to(root)
        backup_file = backup_path / rel_path
        backup_file.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(file_path, backup_file)
        manifest['files'].append(str(rel_path))

    (backup_path / 'manifest.json').write_text(json.dumps(manifest, indent=2))

    return backup_path


def restore_backup(root: Path, backup_path: Path):
    """Restore files from a backup."""
    manifest_path = backup_path / 'manifest.json'
    if not manifest_path.exists():
        raise ValueError(f"No manifest found in {backup_path}")

    manifest = json.loads(manifest_path.read_text())

    for rel_path_str in manifest['files']:
        rel_path = Path(rel_path_str)
        backup_file = backup_path / rel_path
        target_file = root / rel_path

        if backup_file.exists():
            shutil.copy2(backup_file, target_file)
            print(f"  Restored: {rel_path}")


def find_crates(root: Path) -> list[Path]:
    """Find all Rust crates in the project."""
    crates = []
    for item in root.iterdir():
        if item.is_dir() and item.name.startswith('webbook-'):
            if (item / 'Cargo.toml').exists():
                crates.append(item)
            for subdir in item.iterdir():
                if subdir.is_dir() and (subdir / 'Cargo.toml').exists():
                    crates.append(subdir)
    return crates


def cmd_analyze(args, root: Path, crates: list[Path]):
    """Analyze inline tests without modifying anything."""
    print("Analyzing inline test modules...\n")

    total_modules = 0
    total_tests = 0

    for crate in sorted(crates):
        src_dir = crate / 'src'
        if not src_dir.exists():
            continue

        inline_tests = find_inline_tests(src_dir)
        if not inline_tests:
            continue

        crate_name = crate.name
        print(f"{crate_name}: {len(inline_tests)} inline test module(s)")

        for module in inline_tests:
            rel_path = module.file_path.relative_to(root)
            test_count = len(module.test_functions)
            total_tests += test_count
            total_modules += 1

            print(f"  {rel_path}:{module.start_line}-{module.end_line}")
            print(f"    Module: {module.module_name}")
            print(f"    Tests: {test_count}")
            if module.test_functions:
                funcs = ', '.join(module.test_functions[:3])
                if len(module.test_functions) > 3:
                    funcs += f"... (+{len(module.test_functions) - 3} more)"
                print(f"    Functions: {funcs}")

        print()

    print(f"Total: {total_modules} modules, {total_tests} test functions")
    print()
    print("Run 'python3 scripts/refactor_tests.py extract' to extract tests to tests/ directories.")


def cmd_extract(args, root: Path, crates: list[Path]):
    """Extract inline tests to tests/ directories."""
    print("Extracting inline tests to tests/ directories...\n")

    all_files_to_modify = []

    # First pass: collect all files
    for crate in crates:
        src_dir = crate / 'src'
        if not src_dir.exists():
            continue

        inline_tests = find_inline_tests(src_dir)
        for module in inline_tests:
            all_files_to_modify.append(module.file_path)

    if not all_files_to_modify:
        print("No inline test modules found. Nothing to extract.")
        return

    # Create backup
    print(f"Creating backup of {len(all_files_to_modify)} files...")
    backup_path = backup_files(root, all_files_to_modify)
    print(f"Backup created at: {backup_path.relative_to(root)}\n")

    # Second pass: extract and modify
    for crate in sorted(crates):
        src_dir = crate / 'src'
        tests_dir = crate / 'tests'
        crate_name = crate.name

        if not src_dir.exists():
            continue

        inline_tests = find_inline_tests(src_dir)
        if not inline_tests:
            continue

        print(f"{crate_name}: extracting {len(inline_tests)} module(s)")

        # Create tests directory
        tests_dir.mkdir(parents=True, exist_ok=True)

        # Group by source file
        by_file: dict[Path, list[InlineTestModule]] = {}
        for test in inline_tests:
            by_file.setdefault(test.file_path, []).append(test)

        for src_file, modules in by_file.items():
            for module in modules:
                test_name = module.module_name.replace('::', '_')
                test_file = tests_dir / f'{test_name}_tests.rs'

                # Generate new test file content
                new_test_content = generate_integration_test(crate_name, module.module_name, module)

                # Append if file exists
                if test_file.exists():
                    existing = test_file.read_text()
                    new_test_content = existing.rstrip() + '\n\n' + new_test_content

                test_file.write_text(new_test_content)
                rel_test = test_file.relative_to(root)
                print(f"  Created: {rel_test}")

            # Remove inline tests from source
            new_src = remove_inline_tests_from_source(src_file, modules)
            src_file.write_text(new_src)
            rel_src = src_file.relative_to(root)
            print(f"  Modified: {rel_src}")

    print()
    print("Extraction complete!")
    print()
    print("Next steps:")
    print("  1. Run 'cargo test -p webbook-core -p webbook-relay' to check compilation")
    print("  2. If there are errors about private items, add pub(crate) visibility")
    print("  3. Run 'python3 scripts/refactor_tests.py rollback' to undo if needed")


def cmd_verify(args, root: Path, crates: list[Path]):
    """Run cargo test to verify the refactoring."""
    print("Running cargo test to verify...\n")

    result = subprocess.run(
        ['cargo', 'test', '-p', 'webbook-core', '-p', 'webbook-relay'],
        cwd=root,
        capture_output=True,
        text=True
    )

    print(result.stdout)
    if result.stderr:
        print(result.stderr)

    if result.returncode == 0:
        print("\nAll tests passed!")
    else:
        print("\nTests failed. You may need to:")
        print("  - Add pub(crate) visibility to private items used in tests")
        print("  - Run 'python3 scripts/refactor_tests.py rollback' to undo changes")


def cmd_rollback(args, root: Path, crates: list[Path]):
    """Rollback to the most recent backup."""
    backup_root = root / BACKUP_DIR

    if not backup_root.exists():
        print("No backups found.")
        return

    backups = sorted(backup_root.iterdir(), reverse=True)
    if not backups:
        print("No backups found.")
        return

    latest_backup = backups[0]
    print(f"Rolling back to: {latest_backup.name}")

    restore_backup(root, latest_backup)

    print("\nRollback complete!")

    # Clean up backup
    if args.cleanup:
        shutil.rmtree(latest_backup)
        print(f"Removed backup: {latest_backup.name}")


def main():
    parser = argparse.ArgumentParser(
        description='Extract inline tests to tests/ directory',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog='''
Commands:
  analyze    Show what would be extracted without modifying files
  extract    Extract inline tests to tests/ directories (creates backup)
  verify     Run cargo test to verify the refactoring
  rollback   Restore from most recent backup

Examples:
  python3 scripts/refactor_tests.py analyze
  python3 scripts/refactor_tests.py extract
  python3 scripts/refactor_tests.py rollback --cleanup
        '''
    )

    parser.add_argument('command', choices=['analyze', 'extract', 'verify', 'rollback'],
                        help='Command to run')
    parser.add_argument('--crate', type=str, help='Process only a specific crate')
    parser.add_argument('--cleanup', action='store_true', help='Remove backup after rollback')

    args = parser.parse_args()

    root = Path(__file__).parent.parent

    if args.crate:
        crate_path = root / args.crate
        if not crate_path.exists():
            crate_path = root / f'webbook-{args.crate}'
        if not crate_path.exists():
            print(f"Crate not found: {args.crate}")
            return 1
        crates = [crate_path]
    else:
        crates = find_crates(root)

    commands = {
        'analyze': cmd_analyze,
        'extract': cmd_extract,
        'verify': cmd_verify,
        'rollback': cmd_rollback,
    }

    return commands[args.command](args, root, crates)


if __name__ == '__main__':
    exit(main() or 0)
