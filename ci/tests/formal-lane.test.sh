#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# ci/formal-verification.sh runs the multi-stage exchange model beside the
# other models so the job fits its hour (vauchi/private#1296). A lane in
# the background can fail unseen: its exit status is not the script's, and
# its output goes to a file. This runs the `models` set with stub provers
# and checks that
#
# - the lane's runs are printed, after the last model of the set;
# - a multi-stage run that misses its verdict fails the script, names the
#   lemma, and does so only after the other models ran.
#
# The stubs give every lemma both verdicts, so only the script's own flow
# is under test; no prover is started.
#
# Usage: ci/tests/formal-lane.test.sh [formal-script]
#        (from the core repo root)

set -eu

script=${1:-ci/formal-verification.sh}
failures=0
fail() {
    echo "FAIL: $1" >&2
    failures=$((failures + 1))
}

home=$(mktemp -d)
trap 'rm -rf "$home"' EXIT
mkdir -p "$home/bin" "$home/node/bin" "$home/jre/bin" "$home/quint/node_modules/.bin"

cat > "$home/bin/tamarin-prover" << 'EOF'
#!/bin/sh
for arg in "$@"; do
    case "$arg" in *.spthy) model=$arg ;; esac
done
case "$model" in
    *"${STUB_MODEL_WITHOUT_VERDICTS:-none}") exit 0 ;;
esac
for lemma in $(sed -n 's/^lemma \([a-z_]*\).*/\1/p' "$model"); do
    echo "  $lemma (all-traces): verified (1 steps)"
    echo "  $lemma (all-traces): falsified - found trace (1 steps)"
done
echo "  DiffLemma:  Observational_equivalence : verified (1 steps)"
echo "  DiffLemma:  Observational_equivalence : falsified - found trace (1 steps)"
EOF

quint_version=$(sed -n 's/^QUINT_VERSION=//p' "$script")
cat > "$home/quint/node_modules/.bin/quint" << EOF
#!/bin/sh
case "\$1" in --version) echo "$quint_version"; exit 0 ;; esac
echo "[ok]"
echo "[violation]"
echo "  ok stub trace"
echo "1 passed"
EOF

printf '#!/bin/sh\n' > "$home/node/bin/node"
printf '#!/bin/sh\n' > "$home/jre/bin/java"
chmod +x "$home/bin/tamarin-prover" "$home/quint/node_modules/.bin/quint" \
    "$home/node/bin/node" "$home/jre/bin/java"

last_of_the_lane='multistage_exchange.spthy --prove=a_waiting_phone_saves_only_after_the_holders_yes -DREBIND:'
last_of_the_set='device_link_machines.qnt lossy_relay run no_device_registered_for_a_failed_joiner:'
line_of() {
    printf '%s\n' "$2" | grep -nF -- "$1" | head -1 | cut -d: -f1
}

status=0
out=$(FORMAL_SET=models FORMAL_HOME="$home" sh "$script" 2>&1) || status=$?
[ "$status" = 0 ] || fail "the models set with every verdict met exits $status, not 0"
lane_line=$(line_of "$last_of_the_lane" "$out")
set_line=$(line_of "$last_of_the_set" "$out")
if [ -z "$lane_line" ] || [ -z "$set_line" ]; then
    fail "the output lacks the lane's last run or the set's last model"
elif [ "$lane_line" -lt "$set_line" ]; then
    fail "the multi-stage runs are printed before the other models finished: they did not run beside them"
fi
printf '%s\n' "$out" | grep -qF 'formal verification OK (models)' ||
    fail "a passing run does not end with its OK line"

status=0
out=$(STUB_MODEL_WITHOUT_VERDICTS=multistage_exchange.spthy FORMAL_SET=models \
    FORMAL_HOME="$home" sh "$script" 2>&1) || status=$?
[ "$status" != 0 ] || fail "a multi-stage run without its verdict leaves the script at exit 0"
printf '%s\n' "$out" | grep -qF 'FAIL: Tamarin lemma executable is not verified' ||
    fail "the failed multi-stage lemma is not named"
printf '%s\n' "$out" | grep -qF -- "$last_of_the_set" ||
    fail "the other models did not run before the lane's failure was taken"
if printf '%s\n' "$out" | grep -qF 'formal verification OK'; then
    fail "a failed lane still prints the OK line"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed" >&2
    exit 1
fi
echo "PASS: the multi-stage lane is printed after the set and its failure fails the script"
