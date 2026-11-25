# Akkhara `v3.1.2-alpha.3` — ထွက်ရှိမှု မှတ်တမ်း

**ထွက်ရှိသည့် ရက်စွဲ:** ၂၀၂၆ နိုဝင်ဘာ ၂၄
**အထူးအဆင့် (tag):** `v3.1.2-alpha.3`

ဤဗားရှင်းသည် native `akk build` backend ၏ slice ကို နောက်တစ်ဆင့်
တိုးချဲ့ပါသည် — စာရင်း၊ အုပ်စု သို့မဟုတ် အစုအတွက် `slice(xs,
start[, end])` ကို compile လုပ်နိုင်ပြီဖြစ်သည်။ ရလဒ်သည် interpreter
နှင့် stdout၊ stderr နှင့် exit status အတိအကျ တူညီပြီး၊
မထောက်ပံ့နိုင်သေးသည့် ပုံစံများကို `E177`/`E110` ဖြင့် အတိအလင်း
ငြင်းဆိုဆဲ ဖြစ်ပါသည်။

---

## ၁။ `slice` ကို native executable တွင် သုံးနိုင်ပြီ

- `slice(xs, start)` နှင့် `slice(xs, start, end)` ကို စာရင်း၊ အုပ်စု
  (tuple) သို့မဟုတ် အစု (set) အတွက် `akk build` တွင် ယခု သုံးနိုင်ပါပြီ။
  ၎င်းသည် မူရင်းကို မထိဘဲ၊ ရွေးချယ်လိုက်သော ဒြပ်စင်များကို
  စာရင်းအသစ်တစ်ခုအဖြစ် ပြန်ပေးသည်။ ရလဒ်သည် အမြဲ **စာရင်း** ဖြစ်သည် —
  အုပ်စု သို့မဟုတ် အစုမှ slice လုပ်လျှင်လည်း စာရင်း ရသည်၊
  interpreter ၏ အဖြေအတိုင်း ဖြစ်သည်။
- စုစည်းမှု descriptor တွင် ဒြပ်စင်တိုင်း၏ `{ tag, word }` အတွဲ
  ပါပြီးသားဖြစ်သဖြင့် `rt_slice_seq` က ထို အတွဲများကို အစဉ်အတိုင်း
  ကူးယူသည်။ ထို့ကြောင့် အမျိုးအစား မတူသော collection ကိုလည်း
  slice လုပ်နိုင်ပြီး၊ ရလဒ်ထဲတွင် ဒြပ်စင်တိုင်း၏ ကိုယ်ပိုင် tag
  နှင့် nested shape မပျက်ဘဲ ပါလာသည်။
- ရလဒ်သည် စာရင်းအသစ်ဖြစ်သဖြင့် ၎င်းကို ပြင်လိုက်ခြင်းက
  မူရင်း collection ကို မပြောင်းပါ — interpreter ၏ value semantics
  အတိုင်း ဖြစ်သည်။

## ၂။ အကွာအဝေး စည်းမျဉ်းများ — interpreter နှင့် တစ်ထပ်တည်း

- အစ (start) နှင့် အဆုံး (end) နှစ်ခုလုံးကို collection ၏ အရှည်သို့
  **clamp** လုပ်သည်။ `slice(xs, 99)` ကဲ့သို့ အရှည်ကို ကျော်သော
  အစသည် အမှားမဟုတ်ဘဲ အလွတ်စာရင်း ပြန်ပေးသည်။
- အဆုံးကို မပေးလျှင် collection ၏ အရှည်ကို အဆုံးအဖြစ် ယူသည်။
- အစသည် အဆုံးထက် ကြီးလျှင် (`slice(xs, 3, 1)`) အလွတ်စာရင်း ရသည်။
  ဤအစီအစဉ်ကို runtime က တစ်နေရာတည်းတွင် လုပ်သဖြင့်
  နေရာတိုင်း တူညီသည်။
- အနုတ် အစ သို့မဟုတ် အနုတ် အဆုံးသာ မှားသည်။ ၎င်းသည် interpreter ၏
  `E110` ဖြစ်ပြီး၊ executable က ထို စာကြောင်းကို တန်ဖိုးနှစ်ခုလုံး
  ပါအောင် ရေးကာ exit 1 ဖြစ်သည်။ compiled call တွင် span မပါသဖြင့်
  လိုင်းနံပါတ်အပိုင်းသာ မပါပါ။

## ၃။ ငြင်းဆိုမှုများ — တိတ်ဆိတ်စွာ မှားယွင်း မထုတ်ပါ

- စာသား (string) ကို slice လုပ်ခြင်းကို `E177` ဖြင့် ငြင်းသည် —
  interpreter က grapheme cluster အလိုက် ဖြတ်သဖြင့် Unicode table
  လိုအပ်ပြီး native runtime တွင် မပါသေးပါ။
- အဘိဓာန် (dict) သည် စဉ်လိုက် (sequence) မဟုတ်သဖြင့် interpreter ၏
  `E110` ကို လိုင်းနံပါတ်မပါဘဲ ထုတ်သည်။
- argument အရေအတွက် မှား (၁ ခု သို့မဟုတ် ၄ ခု) လျှင်လည်း
  interpreter ၏ `E110` စာသားအတိုင်း ငြင်းသည်။
- အစ/အဆုံး နေရာများသည် int ဖြစ်ရသည်။ `Dynamic` ဒြပ်စင် shape ပါသော
  collection ကို slice လုပ်ခြင်းကို ငြင်းမထားပါ — ကူးယူသည့်အခါ
  tag ပါ လိုက်ပါသဖြင့် ရလဒ်သည် မှန်ကန်သည်။
- `slice` မပါသော အခြား construct များ (library import၊ class၊ closure၊
  destructuring၊ `null`၊ `input()` စသည်) ကို ယခင်အတိုင်း `E177` ဖြင့်
  ဆက်လက် ငြင်းဆဲ ဖြစ်သည်။

## ၄။ တည်ငြိမ်မှုနှင့် parity

- Parity harness — match **၁၅**၊ refuse **၄၂**။ ဤဗားရှင်းတွင်
  `tests/parity/expectations.txt` မပြောင်းပါ (corpus ထဲတွင် `slice`
  ခေါ်သော fixture မရှိပါ)။
- `E177` ၏ slice အကျဉ်းချုပ် စာသားတွင် `slice` ကို ယခု ထည့်ထားသည်။
- Refused ဆဲ စာရင်း (ပြောင်းမထား): dynamic တန်ဖိုးအပေါ် `str`/
  တွက်ချက်မှု/နှိုင်းယှဉ်ချက်/condition၊ run-time string ၏ `len`/index/
  iteration/slice/reverse၊ `contains`/`join`/`split` စသည့် ကျန်
  builtin များ၊ library import နှင့် library call၊ class၊ closure/
  function value၊ destructuring၊ run-time default parameter၊ top-level
  `return`၊ `null`၊ `input()`။

## ၅။ စမ်းသပ်မှု

- `cargo fmt --all -- --check` — clean (exit 0)။
- `cargo test --all-targets` — unit test ၃၁၈၊ `tests/bundle.rs` ၄၂၊
  `tests/cli.rs` ၈၊ `tests/cli_project.rs` ၁၃၊ `tests/docs_collections.rs` ၂၊
  `tests/native/` ၁၃၊ `tests/parity/` ၃ — စုစုပေါင်း **၃၉၉** အားလုံး
  အောင်မြင်၊ ကျရှုံးမှု မရှိ။
- စမ်းသပ်မှု အသစ်များ —
  `tests/native/fixtures/slice.akk` (အစ/အဆုံး နှစ်မျိုး၊ အဆုံး ချန်ထားခြင်း၊
  အရှည်ကျော် clamp၊ အစကျော်ခြင်း၊ ပြောင်းပြန် အကွာအဝေး၊ value semantics၊
  အုပ်စု၊ အစု၊ nested စာရင်း၊ run-time နေရာ — `--opt`/`--no-opt`
  နှစ်မျိုးလုံး)၊ `slice_negative.akk` နှင့်
  `a_slice_with_a_negative_bound_panics_like_the_interpreter` (အနုတ်
  အဆုံး၏ `E110` စာသားအတိအကျ)၊ `slice_arity`/`slice_of_dict` (`E110`) နှင့်
  `string_slice` (`E177`)။
- `tests/native/fixtures/` တွင် stray `.exe` သို့မဟုတ် `build/`
  directory မကျန်ပါ။

## ၆။ သိထားရန် ကန့်သတ်ချက်များ

- GC/collector မရှိသေးသဖြင့် arena သည် ပုံသေ ၂၅၆ KiB ဖြစ်သည်။
  slice တစ်ခုစီသည် စာရင်းအသစ် တစ်ခု allocate လုပ်သဖြင့် ကွင်းအတွင်း
  အလွန်အကျွံ slice လုပ်လျှင် ကုန်ဆုံးမှုကို `E177` ဖြင့် ရိုးသားစွာ
  အစီရင်ခံပြီး exit 1 ဖြစ်သည်။
- slice နေရာများသည် int သာ ဖြစ်ရသည် — interpreter က ကိန်းပြည့်
  ဒဿမကိန်း (`1.0`) ကိုပါ လက်ခံသဖြင့် ထို ပုံစံကို `E177` ဖြင့်
  ငြင်းသည်။
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
akk update v3.1.2-alpha.3
akk --version         # akk 3.1.2-alpha.3
```

> ယခင် ထွက်ရှိမှု မှတ်တမ်းများ —
> [`docs/releases/v3.1.1-alpha.3.md`](docs/releases/v3.1.1-alpha.3.md)၊
> [`docs/releases/v3.1.0-alpha.3.md`](docs/releases/v3.1.0-alpha.3.md)၊
> [`docs/releases/v3.0.0-alpha.3.md`](docs/releases/v3.0.0-alpha.3.md)။
