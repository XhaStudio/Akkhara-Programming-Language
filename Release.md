# Akkhara `v3.1.0-alpha.3` — ထွက်ရှိမှု မှတ်တမ်း

**ထွက်ရှိသည့် ရက်စွဲ:** ၂၀၂၆ နိုဝင်ဘာ ၂၃
**အထူးအဆင့် (tag):** `v3.1.0-alpha.3`

ဤဗားရှင်းသည် native `akk build` backend ကို list builtin ခြောက်ခု —
`push`/`append`၊ `reverse`၊ `pop`၊ `insert`၊ `remove` နှင့် `sort` — ဖြင့်
တိုးချဲ့ပါသည်။ အားလုံးသည် interpreter နှင့် stdout၊ stderr နှင့် exit
status အတိအကျ တူညီပြီး၊ မထောက်ပံ့နိုင်သေးသည့် ပုံစံများကို `E177` ဖြင့်
အတိအလင်း ငြင်းဆိုဆဲ ဖြစ်ပါသည်။ ထို့အပြင် `switch` (ပုံစံနှစ်မျိုးလုံး) နှင့်
`str()` conversion၊ `{…}` interpolation တို့ကိုပါ native executable ထဲ
compile လုပ်နိုင်ပါပြီ။ သုညဖြင့် စားခြင်း/ကြွင်းခြင်း၏ error များ
(`E036`-`E039`) ကိုလည်း `try/catch` ဖြင့် ဖမ်းနိုင်ပါပြီ။

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

## ၃။ `pop`၊ `insert`၊ `remove` နှင့် `sort`

- `pop(xs)` သည် နောက်ဆုံးဒြပ်စင်ကို ဖယ်ပြီး ထိုတန်ဖိုးကို ပြန်ပေးသည်။
- `insert(xs, at, v)` သည် နေရာ `at` တွင် တန်ဖိုး ထည့်ပြီး အရှည်အသစ်ကို
  ပြန်ပေးသည် — `at` သည် အရှည်နှင့် တူနိုင်သည် (အဆုံးတွင် ထပ်ထည့်ခြင်း)။
- `remove(xs, at)` သည် ထိုနေရာရှိ ဒြပ်စင်ကို ဖယ်ပြီး ထိုတန်ဖိုးကို
  ပြန်ပေးသည်။
- `sort(xs)` သည် int၊ float သို့မဟုတ် string စာရင်းကို ငယ်မှကြီး အစဉ်ဖြင့်
  ပြန်ပေးသည်။ Insertion sort ဖြစ်သဖြင့် **stable** ဖြစ်သည် — တူညီသော
  ဒြပ်စင်များ မူလအစဉ်အတိုင်း ရှိနေသည်။
- လေးခုလုံးသည် collection အသစ်တစ်ခု ဖန်တီးပြီး ကိန်းရှင်ထဲ ပြန်ထည့်သဖြင့်
  interpreter ၏ **value semantics** အတိုင်း ဒုတိယ binding သည် မိမိ
  အကြောင်းအရာကို ဆက်ထိန်းသည်။
- အကောင်အထည်ဖော်ပုံ — runtime helpers အသစ် `rt_pop_seq`၊
  `rt_insert_seq`၊ `rt_remove_seq` နှင့် `rt_sort_seq`။ `sort` သည်
  analyzer သတ်မှတ်ထားသော element kind အလိုက် int/float/`rt_cmp_str`
  နှိုင်းယှဉ်မှုကို ရွေးသည်။ အခြား element kind (bool၊ nested collection)
  ကို `E177` ဖြင့် ငြင်းသည် — interpreter က ၎င်းတို့ကို `E042` ဖြင့်
  နှိုင်းယှဉ်သဖြင့် dynamic value word လိုအပ်သည်။
- ဤ runtime call နှစ်ခုတွင် span မသယ်ဆောင်သဖြင့် run-time error နှစ်ခုသည်
  လိုင်းအပိုင်း မပါဘဲ interpreter ၏ စကားလုံးအတိုင်း ရေးသည် —
  အလွတ်စာရင်း `pop` အတွက် `E110 pop() စာရင်းအလွတ်မှ ဖယ်၍မရပါ` နှင့်
  အညွှန်း အပြင်ဘက်အတွက် `E049 အညွှန်း <n> သည် အကွာအဝေးပြင်ပတွင်
  ရှိနေပါသည်။` (အပေါ် ပုဒ် ၆ တွင် ဖော်ပြထားသည့် arithmetic error များက
  မူ လိုင်းနံပါတ်ကို သယ်ဆောင်ပြီး ဖမ်းနိုင်သည်)။

## ၄။ `str()` နှင့် `{…}` interpolation

- `str(x)` (နှင့် မြန်မာ `စာ`/`စာသား`) ကို native executable ထဲ
  compile လုပ်နိုင်ပါပြီ။ int သည် ဒသမ စာသား၊ bool သည် printer သုံးသည့်
  `True`/`False` descriptor၊ string သည် မူရင်းအတိုင်း၊ float သည်
  ရှိပြီးသား တိကျသော print formatter ကို ပြန်သုံးသည် — ထို formatter ၏
  နောက်ဆုံးအဆင့်က `.data` flag တစ်ခုကို ကြည့်ပြီး stdout သို့ မရေးဘဲ
  arena string အသစ်ထဲ ကူးပေးသည်။ ထို့ကြောင့် `str(2.5)` သည် `2.5`၊
  `str(3.0)` သည် `3.0`၊ အနုတ်/သုည/အလွန်ကြီးသော တန်ဖိုးများလည်း
  interpreter စာလုံးအတိုင်း ဖြစ်သည်။
- String literal အတွင်း `{name}` interpolation ကို ပုံစံနှစ်မျိုးလုံးဖြင့်
  ထောက်ပံ့သည်။ Lowering က `interpolate_text` အတိုင်း တိတိကျကျ ဖတ်ပြီး
  (`\{`/`\}` သည် ရိုးရိုး brace၊ brace အတွင်း အမည်ကို trim၊ placeholder
  များကို ရေးထားသည့် အစဉ်အတိုင်း) `str(name)` အပိုင်းများကို `+` ဖြင့်
  ဆက်သည်။ ထို့ကြောင့် `"v = {x}"` နှင့် `"v = " + str(x)` သည်
  တူညီသော ပရိုဂရမ် ဖြစ်သည်။
- Placeholder တစ်ခုသည် `Call` မဟုတ်ဘဲ `MirInst::ToStr` အဖြစ်
  ပြောင်းလဲသဖြင့် အသုံးပြုသူက ရေးထားသော `str` function က
  interpolation ကို လွှမ်းမိုးနိုင်မည် မဟုတ်ပါ။ Analyzer က ၎င်းကို
  string အဖြစ် သတ်မှတ်ပြီး DCE/constant-fold pass များက သယ်ဆောင်ကာ
  codegen helper တစ်ခုတည်းက ထုတ်ပေးသည်။
- `tests/native/fixtures/stringify.akk` — int (အပေါင်း/အနုတ်/သုည/အကြီးဆုံး)၊
  float (ဒသမပါ/ပြည့်/`-0.0`/အလွန်သေး/အလွန်ကြီး)၊ bool နှစ်မျိုး၊ မြန်မာ
  စာသား၊ escaped brace၊ ပေါင်းထားသော conversion နှင့် string/int ဖြင့်
  ခေါ်ထားသော parameter — `--opt` နှင့် `--no-opt` နှစ်မျိုးလုံး
  interpreter output နှင့် byte-by-byte တူညီသည်။

## ၅။ `switch`

- `switch` ကို အင်္ဂလိပ် နှင့် မြန်မာ (`ရွေးရန်`) ပုံစံ နှစ်မျိုးလုံးဖြင့်
  native executable ထဲ compile လုပ်နိုင်ပါပြီ။
- အကောင်အထည်ဖော်ပုံ — lowering က subject ကို temporary တစ်ခုထဲ
  တစ်ကြိမ်တည်း တွက်ပြီး၊ `case` တစ်ခုစီအတွက် `Compare`/`Branch`
  တစ်စုံ ထုတ်ကာ arm တစ်ခုစီကို ခွဲထားသည်။ MIR instruction အသစ်၊
  backend စည်းမျဉ်းအသစ် သို့မဟုတ် runtime helper အသစ် မလိုပါ။
- interpreter ၏ အစဉ်အတိုင်း တိတိကျကျ ဖြစ်သည် — subject ကို တစ်ကြိမ်၊
  `case` ဖော်ပြချက်များကို ရေးထားသော အစဉ်အတိုင်း တစ်ခုချင်း တွက်ပြီး
  ပထမဆုံး ကိုက်ညီသည့်နေရာတွင် ရပ်သည်။ ထို့နောက်ရှိ `case` ကို
  လုံးဝ မတွက်သဖြင့် ၎င်းအတွင်းရှိ ခေါ်ဆိုမှုများ၏ ဘေးထွက်ဆိုးကျိုး
  များလည်း မဖြစ်ပေါ်ပါ။ `default` arm သည် case တစ်ခုမှ မကိုက်ညီမှသာ
  (ရေးထားသည့်နေရာ မည်သို့ဖြစ်စေ) အလုပ်လုပ်သည်၊ fall-through မရှိပါ။
- နှိုင်းယှဉ်မှုသည် ဘာသာစကား၏ `==` ဖြစ်သဖြင့် float subject သည်
  `int` case နှင့် ဘယ်တော့မှ မတူပါ။
- arm အတွင်းရှိ `break` သည် switch ကို ရပ်ပြီး ပတ်လည်ရှိ loop ကို
  မရပ်ပါ (lowering တွင် break-target stack ကို continue target များနှင့်
  ခွဲထားသည်)၊ `continue` မူ loop သို့ ဆက်ရောက်သည်။ loop မပါသော
  switch အတွင်းရှိ `break` သည်လည်း compile ဖြစ်သည်။
- `tests/native/fixtures/switch.akk` — default ကို ရှေးဦးရေးထားမှု၊
  ပထမဆုံး ကိုက်ညီမှုသာ၊ default မပါဘဲ မကိုက်ညီမှု၊ default သာ ပါသော
  switch၊ case ဖော်ပြချက်များ၏ အစဉ်၊ float/int case၊ nested switch၊
  string/bool subject၊ loop အတွင်း `break`/`continue` နှင့် မြန်မာ
  ပုံစံ — `--opt` နှင့် `--no-opt` နှစ်မျိုးလုံး interpreter output နှင့်
  byte-by-byte နှိုင်းယှဉ်ထားသည်။

## ၆။ သုညဖြင့် စားခြင်းကို ဖမ်းနိုင်မှု

- `/` သို့မဟုတ် `%` ၏ စားကိန်း (divisor) သုညဖြစ်ခြင်းသည် ယခု
  executable ကို ရပ်မပစ်တော့ဘဲ raise တစ်ခုကဲ့သို့ ပြေလျော့ (unwind) သည်။
  ထို့ကြောင့် interpreter ၏ `E036`/`E037` (စားခြင်း) နှင့် `E038`/`E039`
  (ကြွင်းကိန်း) ကို `try/catch` ဖြင့် ဖမ်းနိုင်ပါပြီ။
- အကောင်အထည်ဖော်ပုံ — MIR တွင် `guard_zero` instruction အသစ်တစ်ခုကို
  `/`/`%` တစ်ခုစီ၏ အရှေ့တွင် ထုတ်သည်။ ၎င်းသည် operator ၏ လိုင်းနံပါတ်နှင့်
  error ရောက်ရမည့် block (အတွင်းကျဆုံး `try` သို့မဟုတ် function ၏
  escape) ကို သယ်ဆောင်သဖြင့် dead-code ဖယ်ရှားခြင်း၊ block
  ပြန်နံပါတ်တပ်ခြင်းတို့တွင် မပျောက်ပါ။ codegen က operand kind အလိုက်
  code ကို ရွေးသည် — int နှစ်ခုဆိုလျှင် `E036`/`E038`၊ float တစ်ခု
  ပါလျှင် `E037`/`E039`။ float စားကိန်းကို `ucomisd` ဖြင့် စစ်ပြီး `NaN`
  ကို သုညမဟုတ်ဟု သတ်မှတ်သည်။ စာသားသည် interpreter ၏ စာသားအတိုင်း
  လိုင်းနံပါတ်ပါသည်။
- Unwinding protocol — pending flag အဖြစ် ယခု identity slot ကို သုံးသည်။
  ထို့ကြောင့် callee အတွင်း ဖြစ်သော panic သည် caller သို့ raise ကဲ့သို့
  ပြန်တက်သည်။ `E178` သည် raise ကိုသာ ဖမ်းသည် (panic တွင် prefix မရှိ)၊
  `E` သည် နှစ်မျိုးလုံးကို ဖမ်းသည်။ ဖမ်းသည့် clause က bind လုပ်သည့်
  တန်ဖိုးမှာ raise ၏ message သို့မဟုတ် panic ၏ code (`E036`) ဖြစ်သည်။
  မဖမ်းမိလျှင် `rt_report_error` က လိုင်းနံပါတ်ပါ စာသားကို stderr တွင်
  ရေးပြီး exit 1 ဖြစ်သည်။
- ငြင်းဆိုမှု — program တွင် တကယ်ဖြစ်နိုင်သည့် code ကိုသာ clause တွင်
  ရေးနိုင်သည် (`/` ရှိလျှင် `E036`/`E037`၊ `%` ရှိလျှင် `E038`/`E039`)။
  `E104` ကဲ့သို့ အခြား code ကို `E177` ဖြင့် ဆက်လက် ငြင်းသည်။ `x %= 0`
  ကဲ့သို့ compound remainder တွင် guard မရှိသဖြင့် `E038` ဖြင့် exit 1
  ဖြစ်ဆဲ ဖြစ်ပြီး လိုင်းနံပါတ် မပါပါ။

## ၇။ တည်ငြိမ်မှုနှင့် ငြင်းဆိုမှုများ

- Slice အပြင် construct များကို ခန့်မှန်းမနေဘဲ `E177` ဖြင့်
  အတိအလင်း ငြင်းဆိုဆဲ ဖြစ်ပြီး၊ ငြင်းခံရသော ပရိုဂရမ်တစ်ခုမှ
  executable မထုတ်ပါ။ `E177` စာသားတွင် ယခု `push`/`append`၊ `reverse`
  နှင့် `str` conversion ကိုပါ ထောက်ပံ့သည့် စာရင်းအဖြစ် ဖော်ပြထားပါသည်။
- Collection တစ်ခုကို `str()` လုပ်ခြင်း (၎င်း၏ `repr` ကို buffer ထဲ
  ရေးရမည်)၊ `{အတု …}` placeholder၊ မပိတ်ထားသော သို့မဟုတ် အလွတ်
  `{}` နှင့် scope ထဲ မရှိသော interpolation အမည်ကို `E177` ဖြင့်
  ငြင်းသည်။
- Parity harness — match **၁၅**၊ refuse **၄၁**။ `native_interpolation`၊
  `eng_print` နှင့် `switch_forms` (`str(i)` ကို စောင့်နေခဲ့သည်) တို့သည်
  ယခု compile ဖြစ်ပြီး interpreter နှင့် byte-by-byte တူညီသည်။
  `div_zero` နှင့် `mod_zero` သည်လည်း ငြင်းဆိုမှုမှ **match** သို့ ရွေ့ပြောင်းပြီး
  stdout၊ လိုင်းနံပါတ်ပါ stderr နှင့် exit status အားလုံး တူညီသည်။
  `functions`၊ `eng_fn` နှင့် `collections` သည် interpolation ကို
  ကျော်ပြီး သီးခြား gap များတွင် ဆက်လက် ငြင်းခံရသည် — parameter
  တစ်ခု၏ kind မသေချာမှု၊ function တစ်ခု လမ်းတစ်ခုတွင်သာ တန်ဖိုး
  ပြန်ပေးမှု၊ ကိန်းရှင်တစ်ခုကို float အဖြစ်လည်းကောင်း string အဖြစ်လည်းကောင်း
  ပြန်သုံးမှု။ Corpus fixture `mutating` သည် compile ဖြစ်ပြီး
  ကိုက်ညီဆဲ ဖြစ်သည်။ `remaining_forms` သည် `reverse("abc")` တွင်
  ဆက်လက် ငြင်းခံရပါသည်။

## ၈။ စမ်းသပ်မှု

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
- `tests/native/fixtures/list_edits.akk` — `pop`/`insert`/`remove`/`sort`
  တို့၏ ပြန်လာသော တန်ဖိုးများ၊ ရှေ့/အလယ်/အဆုံး နေရာများ၊ ရလဒ်ကို
  မသုံးဘဲ စာရင်းပြောင်းလဲမှု၊ value semantics၊ int/float/string sort —
  `--opt` နှင့် `--no-opt` နှစ်မျိုးလုံး။ `pop_empty` သည် အလွတ် `pop`
  panic ကို စမ်းသည်။
- `tests/native/fixtures/switch.akk` — `switch` lowering ၏ အပြုအမူ
  အားလုံး (အပေါ် ပုဒ် ၅ တွင် ဖော်ပြထားသည့်အတိုင်း)။
- `tests/native/fixtures/stringify.akk` — `str()` နှင့် interpolation ၏
  အပြုအမူအားလုံး (အပေါ် ပုဒ် ၄ တွင် ဖော်ပြထားသည့်အတိုင်း)။
- `cargo fmt --all -- --check` — clean။
- `tests/native/fixtures/arith_catch.akk` — run-time arithmetic error ကို
  raise ကဲ့သို့ ဖမ်းနိုင်မှု (အပေါ် ပုဒ် ၆ တွင် ဖော်ပြထားသည့်အတိုင်း)။
- `cargo test --all-targets` — unit test ၃၁၇၊ `tests/bundle.rs` ၄၂၊
  `tests/cli.rs` ၈၊ `tests/cli_project.rs` ၁၃၊ `tests/docs_collections.rs` ၂၊
  `tests/native/` ၁၁၊ `tests/parity/` ၃ — စုစုပေါင်း **၃၉၆** အားလုံး
  အောင်မြင်၊ ကျရှုံးမှု မရှိ။

## ၉။ သိထားရန် ကန့်သတ်ချက်များ

- native slice အပြင်ဘက် construct များ (closure များ၊ library import၊
  run-time string ၏ `len`/index/iteration၊
  heterogeneous collection များ၊ `slice`/`sum`/`join`/`contains` စသည့်
  ကျန် collection builtin များ) ကို `E177` ဖြင့် ဆက်လက် ငြင်းဆဲ ဖြစ်သည်။
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
