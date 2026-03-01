<!-- SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# CLAUDE.md - Core Build Scripts

Build/test scripts for vauchi-core and vauchi-mobile. Run from `core/` directory.
CI release flow: `build-bindings.sh → package-*.sh → publish-packages.sh → trigger-downstream.sh`
