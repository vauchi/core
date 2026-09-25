// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

// Image bytes serialize as one number per line: the onboarding mark alone
// turned a 16 KB fixture into 400 KB, and every shell binary embeds the
// checked-in fixtures.
pub fn collapse_number_arrays(pretty: &str) -> String {
    let lines: Vec<&str> = pretty.lines().collect();
    let mut out = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.ends_with('[') {
            let numbers: Vec<&str> = lines[i + 1..]
                .iter()
                .map(|l| l.trim().trim_end_matches(','))
                .take_while(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()))
                .collect();
            let close = lines.get(i + 1 + numbers.len()).map(|l| l.trim_start());
            if !numbers.is_empty() && close.is_some_and(|l| l.starts_with(']')) {
                out.push(format!(
                    "{line}{}{}",
                    numbers.join(","),
                    close.unwrap_or_default()
                ));
                i += numbers.len() + 2;
                continue;
            }
        }
        out.push(line.to_owned());
        i += 1;
    }
    out.join("\n")
}
