# Akkhara `v3.1.4-alpha.3` — ထွက်ရှိမှု မှတ်တမ်း

**ထွက်ရှိသည့် ရက်စွဲ:** ၂၀၂၆ နိုဝင်ဘာ ၂၅
**အထူးအဆင့် (tag):** `v3.1.4-alpha.3`

ဤဗားရှင်းသည် native `akk build` backend ၏ slice ကို နောက်တစ်ဆင့်
တိုးချဲ့ပါသည် — `trim(text)` ကို executable ထဲတွင် တိုက်ရိုက်
တွက်ချက်နိုင်ပြီဖြစ်သည်။ ၎င်းသည် Unicode စည်းမျဉ်းကို လိုအပ်သော
ပထမဆုံး စာသား builtin ဖြစ်ပြီး၊ ရလဒ်သည် interpreter နှင့် stdout၊
stderr နှင့် exit status အတိအကျ တူညီပါသည်။

---

## ၁။ `trim` ကို native executable တွင် သုံးနိုင်ပြီ

- `trim(text)` သည် စာသား၏ အစနှင့် အဆုံးမှ Unicode whitespace များကို
  ဖယ်ရှားပြီး ကျန်စာသားကို ပြန်ပေးသည် — interpreter ၏ `str::trim`
  အတိအကျ ဖြစ်သည်။
- runtime က UTF-8 ကို အစမှ တစ် scalar ချင်း၊ အဆုံးမှ နောက်ပြန်
  တစ် scalar ချင်း လျှောက်သည် (`rt_trim_str`)။ scalar တစ်ခုသည်
  whitespace ဟုတ်မဟုတ်ကို `rt_ws_len` က စစ်သည်။
- **အရေးကြီးချက်:** ထို whitespace စာရင်းကို ကျွန်ုပ်တို့ လက်ဖြင့်
  ရေးထည့်ထားခြင်း မဟုတ်ပါ။ compile လုပ်ချိန်တွင် host ၏
  `char::is_whitespace` မှ တိုက်ရိုက် ထုတ်ယူပြီး byte နှိုင်းယှဉ်ချက်
  အဖြစ် ရေးထည့်ထားသဖြင့် interpreter သုံးသည့် စာရင်းနှင့် တစ်ထပ်တည်း
  ဖြစ်သည် — သာမန် space၊ tab၊ no-break space (U+00A0)၊ ideographic
  space (U+3000) စသည်တို့ အားလုံး ပါဝင်သည်။
- multi-byte scalar တစ်ခုကို တစ်ပိုင်းတစ်စ ဖြတ်မချပါ — မြန်မာ
  စာလုံးတစ်လုံး (consonant + medial + vowel + tone) သည် အစုံလိုက်သာ
  ကျန်သည် သို့မဟုတ် အစုံလိုက်သာ ဖယ်ခံရသည်။
- ဖယ်ရှားစရာ မရှိသော စာသားဆိုလျှင် မူလ string ကိုပင် ပြန်ပေးသည် —
  string များသည် ဤ backend တွင် ပြောင်းလဲ၍မရသဖြင့် မှန်ကန်ပြီး
  memory အသစ် မကုန်ပါ။

## ၂။ ဥပမာ

```
print(trim("  Akkhara  "));      # Akkhara
print("[" + trim("   q   ") + "]");  # [q]
print(trim("\t tab \n"));        # tab
```

## ၃။ ငြင်းဆိုမှုများ — မပြောင်းလဲ

- argument အရေအတွက် မှား (`trim()`) သို့မဟုတ် စာသား မဟုတ်သော
  argument (`trim(1)`) သည် interpreter ၏ `E110` ဖြစ်ပြီး executable
  မထုတ်မီ ငြင်းဆိုသည်။
- `find`/`index_of` ကို ဤဗားရှင်းတွင် မထည့်ပါ။ ၎င်းတို့၏ အဖြေသည်
  **grapheme cluster အညွှန်း** ဖြစ်သဖြင့် syllable table လိုအပ်သည်။
  `upper`/`lower` သည်လည်း ပြည့်စုံသော Unicode case table လိုအပ်သဖြင့်
  နောက် slice အတွက် ချန်ထားသည်။
- library import၊ class၊ closure၊ destructuring၊ `null`၊ `input()` စသည်
  ကို ယခင်အတိုင်း `E177` ဖြင့် ဆက်လက် ငြင်းဆဲ ဖြစ်သည်။

## ၄။ တည်ငြိမ်မှုနှင့် parity

- Parity harness — match **၁၅**၊ refuse **၄၁**။ `tests/parity/expectations.txt`
  မပြောင်းပါ (corpus ထဲတွင် `trim` ခေါ်သော fixture မရှိပါ)။
- `E177` ၏ အကျဉ်းချုပ် စာသားတွင် `trim` ကို ယခု ထည့်ထားသည်။
  `docs/COLLECTIONS.md` ထဲက pinned စာကြောင်းကိုပါ တစ်ခါတည်း
  update လုပ်ထားသည်။

## ၅။ စမ်းသပ်မှု

- `cargo fmt --all -- --check` — clean (exit 0)။
- `cargo test --all-targets` — unit test ၃၁၈၊ `tests/bundle.rs` ၄၂၊
  `tests/cli.rs` ၈၊ `tests/cli_project.rs` ၁၃၊ `tests/docs_collections.rs` ၂၊
  `tests/native/` ၁၅၊ `tests/parity/` ၃ — စုစုပေါင်း **၄၀၁** အားလုံး
  အောင်မြင်၊ ကျရှုံးမှု မရှိ။
- စမ်းသပ်မှု အသစ်များ — `tests/native/fixtures/trim.akk` (space၊ tab နှင့်
  newline၊ အလွတ်စာသား၊ space ချည်း၊ binding၊ parameter၊ စာရင်း ဒြပ်စင်၊
  no-break space၊ ideographic space — `--opt`/`--no-opt` နှစ်မျိုးလုံး)၊
  `trim_arity`/`trim_of_int` (`E110`)။
- Native fixture ၄၉ ခု။ `tests/native/fixtures/` တွင် stray `.exe`
  သို့မဟုတ် `build/` directory မကျန်ပါ။

## ၆။ သိထားရန် ကန့်သတ်ချက်များ

- GC/collector မရှိသေးသဖြင့် arena သည် ပုံသေ ၂၅၆ KiB ဖြစ်သည်။
  `trim` သည် အသစ် string တစ်ခု allocate လုပ်သဖြင့် ကွင်းအတွင်း
  အလွန်အကျွံ ခေါ်လျှင် ကုန်ဆုံးမှုကို `E177` ဖြင့် ရိုးသားစွာ
  အစီရင်ခံပြီး exit 1 ဖြစ်သည်။
- ဖယ်ရှားစရာ မရှိသော စာသားတွင် allocate မလုပ်ပါ (မူလ string ပြန်ပေး)။
- `find`/`index_of`၊ `upper`/`lower` နှင့် run-time string ၏
  `len`/index/iteration/slice/reverse တို့သည် ဤ slice တွင် မပါသေးပါ။
- dynamic တန်ဖိုးအပေါ် `str`/တွက်ချက်မှု dispatch လုပ်နိုင်ရန်
  တစ်လုံးတည်း NaN-boxed value သည် နောက် slice ဖြစ်သည်။
- Linux ELF output ကို ဤ host တွင် မ run နိုင်သေးပါ။ Windows `.exe`
  ကိုသာ တကယ် run စမ်းထားသည်။
- aarch64၊ Mach-O၊ register allocation၊ inlining နှင့် benchmarks
  မပါသေးပါ။

---

## အဆင့်မြှင့်နည်း

```bash
akk update            # နောက်ဆုံး ဗားရှင်းသို့
akk update v3.1.4-alpha.3
akk --version         # akk 3.1.4-alpha.3
```

> ယခင် ထွက်ရှိမှု မှတ်တမ်းများ —
> [`docs/releases/v3.1.3-alpha.3.md`](docs/releases/v3.1.3-alpha.3.md)၊
> [`docs/releases/v3.1.2-alpha.3.md`](docs/releases/v3.1.2-alpha.3.md)၊
> [`docs/releases/v3.1.1-alpha.3.md`](docs/releases/v3.1.1-alpha.3.md)၊
> [`docs/releases/v3.1.0-alpha.3.md`](docs/releases/v3.1.0-alpha.3.md)၊
> [`docs/releases/v3.0.0-alpha.3.md`](docs/releases/v3.0.0-alpha.3.md)။
