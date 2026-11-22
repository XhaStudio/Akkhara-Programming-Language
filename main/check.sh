#!/usr/bin/env bash
# repo root မှာ run ပါ။ အဆင့်တစ်ခု ကျလည်း ရှေ့ဆက်သွားပြီး အဆုံးမှာ အနှစ်ချုပ်ထုတ်ပေးမယ်။
set -u
LOG=/tmp/akk-check; mkdir -p "$LOG"
ROOT="$PWD"
declare -A R

run() {  # run <name> <command...>
  local name="$1"; shift
  "$@" > "$LOG/$name.log" 2>&1
  R[$name]=$?
  echo "--- $name: exit=${R[$name]}"
  grep -E "^test result|FAILED|failed|panicked|^error" "$LOG/$name.log" | head -n 25
}

echo "=== env"
uname -a
rustc --version 2>&1; cargo --version 2>&1; cc --version 2>&1 | head -n 1
for t in perl make pkg-config; do printf "%s: " "$t"; command -v "$t" || echo MISSING; done
nproc 2>/dev/null; free -m 2>/dev/null | head -n 2

echo "=== build + tests (debug)"
run build        cargo build
if [ "${R[build]}" != 0 ]; then echo "build failed: tail of log"; tail -n 30 "$LOG/build.log"; exit 1; fi
run test_bins    cargo test --bins
run test_cli     cargo test --test cli
run test_project cargo test --test cli_project
run test_bundle  cargo test --test bundle

echo "=== manual checks"
AKK="$ROOT/target/debug/akk"
T="$(mktemp -d)"; cd "$T" || exit 1
"$AKK" new demo; cd demo || exit 1

echo "--- akk run"
"$AKK" run; echo "exit=$?"

echo "--- akk build"
"$AKK" build; echo "exit=$?"
ls -l build

echo "--- built exe"
./build/demo; echo "exit=$?"
echo "--- argv ignored (update / --version / help)"
./build/demo update; echo "exit=$?"
./build/demo --version; echo "exit=$?"
./build/demo help; echo "exit=$?"
echo "--- no libraries folder beside exe?"
ls -d build/libraries 2>&1

echo "--- slot count in built exe and in akk (each should be 1)"
grep -aob AKKSLOT1 build/demo | wc -l
grep -aob AKKSLOT1 "$AKK" | wc -l

echo "--- other folder, no akk on PATH"
mkdir -p "$T/other"; cp build/demo "$T/other/"
( cd "$T/other" && env -i PATH=/usr/bin:/bin ./demo; echo "exit=$?"; ls -l )

echo "--- truncated exe (expect E159, no program output)"
cp build/demo build/cut; truncate -s -31 build/cut; ./build/cut; echo "exit=$?"

echo "--- corrupted byte (expect E159 or E160)"
cp build/demo build/bad
size=$(stat -c%s build/bad)
printf '\xff' | dd of=build/bad bs=1 seek=$((size-100)) conv=notrunc 2>/dev/null
./build/bad; echo "exit=$?"

echo "--- build error cases"
printf '[package]\nname = "x"\n[build]\nmode = "bytecode"\n' > "$T/bc.toml"
mkdir -p "$T/p2/src"; cp "$T/bc.toml" "$T/p2/akk.toml"; echo '"hi" ကို ပြပါ။' > "$T/p2/src/main.akk"
( cd "$T/p2" && "$AKK" build; echo "exit=$?" )
mkdir -p "$T/empty"; ( cd "$T/empty" && "$AKK" build; echo "exit=$?" )

echo "=== SUMMARY"
for k in build test_bins test_cli test_project test_bundle; do echo "$k exit=${R[$k]}"; done
echo "logs in $LOG"