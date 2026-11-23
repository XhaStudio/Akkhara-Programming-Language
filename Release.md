# Akkhara `v3.1.0-alpha.3` — ထွက်ရှိမှု မှတ်တမ်း

**ထွက်ရှိသည့် ရက်စွဲ:** ၂၀၂၆ နိုဝင်ဘာ ၂၃
**အထူးအဆင့် (tag):** `v3.1.0-alpha.3`

ဤဗားရှင်းသည် native `akk build` backend ကို list builtin နှစ်ခုဖြစ်သော
`push`/`append` နှင့် `reverse` ဖြင့် တိုးချဲ့ပါသည်။ နှစ်ခုလုံးသည်
interpreter နှင့် stdout၊ stderr နှင့် exit status အတိအကျ တူညီပြီး၊
မထောက်ပံ့နိုင်သေးသည့် ပုံစံများကို `E177` ဖြင့် အတိအလင်း ငြင်းဆိုဆဲ
ဖြစ်ပါသည်။

---

## ၁။ `push` နှင့် `append`

- `push(xs, v)` နှင့် ၎င်း၏ အမည်တူ `append` သည် စာရင်းတစ်ခုသို့ တန်ဖိုး
  တစ်ခု ထည့်ပြီး စာရင်း၏ အရှည်အသစ်ကို `int` အဖြစ် ပြန်ပေးပါသည်။
- ပထမ အချက်သည် စာရင်းကို ကိုင်ထားသော **ကိန်းရှင်** ဖြစ်ရပါမည် —
  interpreter အတိုင်း။ တန်ဖိုးတစ်ခုသည် မိတ္တူ (copy) ဖြစ်သဖြင့်
  `push([1, 2], 3)` သည် ဘယ်နေရာကိုမှ ပြန်ရေးနိုင်မည် မဟုတ်ပါ။
- အကောင်အထည်ဖော်ပုံ — collection runtime ၏ indexed write ကို
  အညွှန်းအနေဖြင့် descriptor ၏ length word ကို ပေး၍ ခေါ်ပါသည်။
  ထို့ကြောင့် `xs[len(xs)] = v` နှင့် လုံးဝ တူညီသော allocation/copy
  လမ်းကြောင်းကို သုံးပြီး၊ ပြန်လာသော descriptor အသစ်သည် binding ကို
  အစားထိုးပါသည်။
- ဤအစားထိုးမှုက interpreter ၏ **value semantics** ကို ထိန်းသည် —
  `ys = xs; push(xs, 6);` ပြီးနောက် `ys` သည် မိမိ၏ မူလအကြောင်းအရာကို
  ဆက်လက် ပြပါသည်။
- `push` သည် စာရင်း၏ element shape ထဲသို့ တန်ဖိုးကို ပေါင်းထည့်သည်။
  မကိုက်ညီသော kind (ဥပမာ int စာရင်းထဲသို့ string) ကို element write
  တစ်ခု ရရှိမည့် စကားလုံးအတိုင်း `E177` ဖြင့် ငြင်းပါသည်။
- အရေအတွက် မှားခြင်း၊ ပထမ အချက်သည် ကိန်းရှင် မဟုတ်ခြင်းနှင့်
  စာရင်း မဟုတ်သော တန်ဖိုးတို့အတွက် interpreter ၏ `E110` ကို
  လိုင်းနံပါတ်သာ ချန်လှပ်၍ တိတိကျကျ ပြန်ထုတ်ပါသည်။

## ၂။ `reverse`

- `reverse(x)` သည် စာရင်း သို့မဟုတ် အစု (tuple) တစ်ခုကို ဒြပ်စင်အစဉ်
  ပြောင်းပြန်ဖြစ်သော descriptor အသစ်အဖြစ် ပြန်ပေးပါသည်။
- အကောင်အထည်ဖော်ပုံ — runtime ၏ `rt_reverse_seq` အသစ်သည် အရွယ်တူ
  descriptor ကို allocate လုပ်၍ header ကို ကူးယူပြီး ဒြပ်စင် `i` ကို
  `len - 1 - i` နေရာသို့ ရွှေ့ပါသည်။ **မူလ descriptor ကို ဘယ်တော့မှ
  မရေးပါ**။
- ကိန်းရှင်တစ်ခုထဲ ရှိသော စာရင်းကို နေရာတွင်တင် ပြောင်းပြန်လုပ်ပြီး
  binding သည် descriptor အသစ်ကို လက်ခံသည်။ အစု သို့မဟုတ် အခြား
  စာရင်းတန်ဖိုးကို တန်ဖိုးအနေဖြင့်သာ ပြောင်းပြန်လုပ်သဖြင့် binding
  မပြောင်းလဲပါ — interpreter ၏ `call_reverse` နှင့် တိတိကျကျ တူသည်။
- စာသား (string) ကို ပြောင်းပြန်လုပ်ခြင်းကို `E177` ဖြင့် ဆက်လက်
  ငြင်းပါသည် — ၎င်းသည် grapheme cluster အလိုက် လုပ်ရသဖြင့် native
  runtime မသယ်ဆောင်သေးသော Unicode tables လိုအပ်သည်။
- အရေအတွက် မှားခြင်းနှင့် စာရင်း/အစု မဟုတ်သော တန်ဖိုးတို့အတွက်
  interpreter ၏ `E110` ကို ပြန်ထုတ်ပါသည်။

## ၃။ တည်ငြိမ်မှုနှင့် ငြင်းဆိုမှုများ

- Slice အပြင် construct များကို ခန့်မှန်းမနေဘဲ `E177` ဖြင့်
  အတိအလင်း ငြင်းဆိုဆဲ ဖြစ်ပြီး၊ ငြင်းခံရသော ပရိုဂရမ်တစ်ခုမှ
  executable မထုတ်ပါ။ `E177` စာသားတွင် ယခု `push`/`append` နှင့်
  `reverse` ကိုပါ ထောက်ပံ့သည့် စာရင်းအဖြစ် ဖော်ပြထားပါသည်။
- Parity harness — match **၉**၊ refuse **၄၇**။ Corpus fixture `mutating`
  သည် ယခင် `push` တွင် ငြင်းခံရာမှ ယခု နောက်ကျသော `pop` တွင်
  ငြင်းခံရသဖြင့် ၎င်း၏ expectation keyword သာ ရွှေ့သွားပြီး
  အရေအတွက် မပြောင်းပါ။ `remaining_forms` သည် `reverse("abc")` တွင်
  ဆက်လက် ငြင်းခံရပါသည်။

## ၄။ စမ်းသပ်မှု

- `tests/native/fixtures/list_builtins.akk` — ပြန်လာသော အရှည်၊
  `while` ကွင်းအတွင်း စာရင်း ကြီးထွားမှု၊ float စာရင်းနှင့် nested
  စာရင်း၊ ကိန်းရှင် နှစ်ခုကြား value semantics၊ ကိန်းရှင်တွင်
  နေရာတွင်တင် ပြောင်းပြန်လုပ်ခြင်း၊ အစု၊ list literal နှင့်
  ပြန်လာသော တန်ဖိုးကို binding အသစ်သို့ ယူခြင်း — `--opt` နှင့်
  `--no-opt` နှစ်မျိုးလုံးအတွက် binary တကယ် run ပြီး interpreter
  output နှင့် byte-by-byte နှိုင်းယှဉ်သည်။
- `push_mixed` — element shape မကိုက်ညီသော `push` ကို `E177` ဖြင့်
  ငြင်းဆိုမှု။ `push_arity` နှင့် `reverse_arity` — interpreter ၏
  `E110` နှင့် ကိုက်ညီမှု။
- `cargo fmt --all -- --check` — clean။
- `cargo test --all-targets` — unit test ၃၁၁၊ `tests/bundle.rs` ၄၂၊
  `tests/cli.rs` ၈၊ `tests/cli_project.rs` ၁၃၊ `tests/docs_collections.rs` ၂၊
  `tests/native/` ၁၀၊ `tests/parity/` ၃ — စုစုပေါင်း **၃၈၉** အားလုံး
  အောင်မြင်၊ ကျရှုံးမှု မရှိ။

## ၅။ သိထားရန် ကန့်သတ်ချက်များ

- native slice အပြင်ဘက် construct များ (closure များ၊ library import၊
  `{…}` interpolation၊ `switch`၊ run-time string ၏ `len`/index/iteration၊
  heterogeneous collection များ၊ `pop`/`insert`/`remove`/`sort` စသည့်
  ကျန် mutating builtin များ) ကို `E177` ဖြင့် ဆက်လက် ငြင်းဆဲ ဖြစ်သည်။
- GC မရှိသေးသဖြင့် ကွင်းအတွင်း စာရင်း အလွန်အကျွံ ကြီးထွားလျှင်
  arena ကုန်ဆုံးမှုကို `E177` ဖြင့် ရိုးသားစွာ အစီရင်ခံပြီး exit 1
  ဖြစ်သည်။
- Linux ELF output ကို ဤ host တွင် မ run နိုင်သေးပါ။ Windows `.exe`
  ကိုသာ တကယ် run စမ်းထားသည်။

---

## အဆင့်မြှင့်နည်း

```bash
akk update            # နောက်ဆုံး ဗားရှင်းသို့
akk update v3.1.0-alpha.3
akk --version         # akk 3.1.0-alpha.3
```

> ယခင် ထွက်ရှိမှု မှတ်တမ်း — [`docs/releases/v3.0.0-alpha.3.md`](docs/releases/v3.0.0-alpha.3.md)။
