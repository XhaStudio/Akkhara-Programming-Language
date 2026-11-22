# Akkhara `v3.0.0-alpha.3` — ထွက်ရှိမှု မှတ်တမ်း

**ထွက်ရှိသည့် ရက်စွဲ:** ၂၀၂၆ နိုဝင်ဘာ ၂၀
**အထူးအဆင့် (tag):** `v3.0.0-alpha.3`

ဤဗားရှင်းသည် native x86-64 backend ကို float နှင့် true division အထိ တိုးချဲ့ပြီး၊ `akk run` ကို interpreter မှ ခွဲထုတ်ကာ compiled executable ကိုသာ ဖွင့်စေသည့် ဗားရှင်း ဖြစ်ပါသည်။

---

## ၁။ Native backend — float နှင့် true division

- Float literal, float ကိန်းရှင်/parameter/return တန်ဖိုးများ၊ `+ - * /` (int/int ပါ Float အဖြစ် ရလာသည်)၊ float နှိုင်းယှဉ်ချက်များနှင့် float ပြသမှုကို `akk build` က တကယ် compile လုပ်ပေးပါပြီ။
- ဂဏန်းတန်ဖိုးကို slot တစ်ခုတည်းထဲ `f64` bit အဖြစ် သယ်ဆောင်သဖြင့် calling convention မပြောင်းလဲပါ။ တွက်ချက်မှုကို `xmm` register များသို့ ရွှေ့ပြီး SSE (`cvtsi2sd`, `addsd`, `subsd`, `mulsd`, `divsd`, `ucomisd`) ဖြင့် လုပ်ပါသည်။
- `==`/`!=` သည် interpreter ၏ သဘောတရားအတိုင်း — int နှင့် float ဘယ်တော့မှ မတူ၊ `-0.0 != 0.0`၊ `NaN == NaN`။ အစဉ်လိုက် နှိုင်းယှဉ်ရာတွင် ရောနှောထားသော အတွဲကို promote လုပ်ပါသည်။
- သုညဖြင့် စားလျှင် trap မဖြစ်ဘဲ interpreter ၏ error code အတိုင်း panic ဖြစ်သည် — int နှစ်ခုအတွက် `E036`၊ float ပါဝင်လျှင် `E037`။ float အပေါ် `%` ကို `E177` ဖြင့် ငြင်းပါသည်။
- Float ပြသမှုသည် interpreter နှင့် byte-identical — bit များမှ ဒသမ ကိန်းဂဏန်းများကို base-10 bignum ဖြင့် တိတိကျကျ ထုတ်ပြီး ပြန်ဖတ်လျှင် မူလတန်ဖိုး ရစေမည့် အတိုဆုံး စာသားကို ရွေးသည်။ ထို့ကြောင့် `0.1 + 0.2` က `0.30000000000000004` ဟု ပြပါသည်။
- Kind analysis သည် `float` ကို နှစ်ဖက်စလုံးမှ သင်ယူနိုင်ပြီး၊ အဆုံးသတ်ချိန်တွင် ကျန်နေသည်များကို `int` default ပေး၍ နောက်တစ်ပတ် ပြန်လည် တွက်သည်။ ထို့ကြောင့် type မှတ်ချက် မပါဘဲ `fn add(a, b) { return a + b }` လည်း compile ဖြစ်ပါသည်။

### ဂဏန်း builtin များ — `abs`, `min`, `max`

- `abs(x)` သည် argument ၏ kind ကို ထိန်းထားသည် — int ကို အနုတ်ဖြစ်နေလျှင် ပြောင်း၍၊ float ၏ sign bit ကို ဖျက်သည်။ ထို့ကြောင့် `abs(-0.0)` က `0.0` ဖြစ်ပြီး NaN သည် NaN ပဲ ဖြစ်နေသည်။
- `min`/`max` သည် candidate တစ်ခုမှ ခြောက်ခုအထိ ယူသည်၊ ဘယ်မှ ညာသို့ အစဉ်လိုက် တွက်သည် (argument ထဲရှိ function ခေါ်မှုလည်း အစဉ်လိုက် လုပ်သည်)၊ ပထမတန်ဖိုးကို ဆက်ထားပြီး တိတိကျကျ သေးငယ်/ကြီးမှသာ အစားထိုးသည် — interpreter ၏ အစဉ်လိုက်စည်းမှန်းချက်တွင် NaN အတွဲသည် ညီသည်ဟု သတ်မှတ်သဖြင့် unordered နှိုင်းယှဉ်မှုက ဘယ်တော့မှ အစားမထိုးပါ။
- ရလဒ် kind သည် candidate များ၏ kind ဖြစ်သဖြင့် slice တွင် kind တစ်မျိုးတည်း လိုအပ်သည် — `min(3, 1, 2)` က int၊ `min(1, 2.5)` ကို `E177` ဖြင့် ငြင်းသည်။ `abs`/`min`/`max` အမည်ရှိ user function ရှိလျှင် ထို function က ဦးစားပေးသည် (interpreter အတိုင်း)။
- ဤအတွက် `tests/native/fixtures/builtins.akk` ကို `--no-opt` ပါ နှစ်မျိုးလုံး စမ်းထားပြီး၊ ရောနှောထားသော `min` ကို ငြင်းသည်ကိုပါ စမ်းထားသည်။

### အဝိုင်းချ builtin များ — `floor`, `ceil`, `round`

- တစ်ခုစီသည် ကိန်းတစ်ခု ယူပြီး `int` ပြန်ပေးသည် — interpreter ၏ saturating `as i64` cast အတိုင်း။ ထို့ကြောင့် NaN က `0`၊ `2^63` နှင့်အထက် က `i64::MAX`၊ `-2^63` အောက် က `i64::MIN` ဖြစ်သည်။
- Int argument ကို အရင် double အဖြစ် ပြောင်းသဖြင့် `floor(9007199254740993)` က `9007199254740992` ဖြစ်သည် — interpreter နှင့် တူညီသည်။
- အကောင်အထည်ဖော်ပုံ — အတိုင်းအတာပြင်ပ တန်ဖိုးများကို ခွဲထုတ်ပြီးမှ `cvttsd2si` ဖြင့် ဖြတ်၍၊ အပိုင်းကိန်းကို ကြည့်ပြီး ချန်သည် (floor အတွက် အနုတ်၊ ceil အတွက် အပေါင်း၊ round အတွက် သုညမှ ဝေးရာ ဝက်)။
- Argument အမျိုးအစား/အရေအတွက် မှားလျှင် interpreter ၏ `E110` ကိုပဲ (လိုင်းနံပါတ် မပါဘဲ) ပြန်ပေးသည်။ `sqrt` က slice ပြင်ပတွင် ဆက်ရှိနေသည် — ၎င်း၏ `E111` message တွင် ဖြစ်စေသည့် တန်ဖိုးကို ထည့်ပြရသဖြင့် compiled code က ထုတ်မပြနိုင်ပါ။
- `tests/native/fixtures/rounding.akk` (အပေါင်း/အနုတ်/ဝက်/int/2^53 ကျော်/parameter မှတစ်ဆင့်) နှင့် `builtin_misuse_reports_the_interpreters_code` (E110 ကိုက်ညီမှု ၄ ခု) တို့ဖြင့် စမ်းထားသည်။

## ၂။ Native backend — function များ

- Top-level `fn` မည်မျှပါပါ compile လုပ်ပေးပါပြီ။ function တစ်ခုစီတွင် parameter၊ binding နှင့် temporary တစ်ခုစီအတွက် slot ပါသော stack frame ရှိသည်၊ argument ခြောက်ခုအထိ ယူနိုင်သည်၊ တန်ဖိုးကို `rax` ဖြင့် ပြန်ပေးသည်။
- ခေါ်ဆိုမှုများကို argument အဖြစ် ထပ်ဆင့်နိုင်သည်၊ နောက်မှ သတ်မှတ်ထားသော function ကို ရည်ညွှန်းနိုင်သည်၊ ကိုယ်တိုင်ခေါ်နိုင်သည် (recursion)၊ အပြန်အလှန် ခေါ်နိုင်သည် (mutual recursion)။
- Convention ကို `docs/ARCHITECTURE.md` တွင် မှတ်တမ်းတင်ထားသည် — argument များ `rdi`/`rsi`/`rdx`/`rcx`/`r8`/`r9`၊ ရလဒ် `rax`၊ `rbp` သာ callee-saved၊ call တိုင်းတွင် `rsp` သည် 16-byte aligned။
- Parameter နှင့် result ၏ kind ကို program တစ်ခုလုံးအတွင်း တွက်ယူပြီး မသိနိုင်လျှင် ခန့်မှန်းမနေဘဲ `E177` ဖြင့် ငြင်းပါသည်။

## ၃။ `akk run` သည် interpreter မသုံးတော့ပါ

- `akk run` သည် source ကို တိုက်ရိုက် interpret လုပ်ခြင်း လုံးဝ မရှိတော့ဘဲ compiled executable ကိုသာ ဖွင့်ပါသည်။
- ဖိုင်တစ်ခုအတွက် compiled ဖိုင်ကို source ဘေးရှိ **`build`** ဖိုဒါထဲတွင် `build/<file_name>.exe` အဖြစ် သိမ်းသည်။ `build` ဖိုဒါ မရှိလျှင်၊ ဖိုင် မရှိလျှင်၊ သို့မဟုတ် ဖိုင်သည် source/import/dependency/`akk.toml`/`akk` ကိုယ်တိုင်ထက် ဟောင်းနေလျှင် — အရင် compile လုပ်ပြီး ရလာသည့် executable ကို ဖွင့်သည်။ အားလုံး current ဖြစ်လျှင် တိုက်ရိုက် ဖွင့်သည်။
- Project တစ်ခုအတွက် manifest ၏ `[build] out_dir` (default `build/`) နှင့် `[build] output` ကို သုံးသည်။
- `.akop` transcript ကို executable ဘေးတွင် ရေးသည် — ယခု `build/hello.akop`။
- **`Interpreted in [NUM]`** လိုင်းကို လုံးဝ ဖျက်ပြီး။ Run တစ်ခု၏ နောက်ဆုံး output သည် program ကိုယ်တိုင် ပြသည့် စာသားပဲ ဖြစ်သည်။

## ၄။ UI — header နှင့် styling

```
Compiling demo 0.1.0 (C:\path\to\demo)
Building [##########] 100% (3/3)
Compiled in 0.043s
Finished C:\path\to\demo\build\demo.exe (257814613 bytes)
```

- `Compiling` — **bold**၊ ဖိုဒါအမည် — **bold**၊ version — **bold + အပြာ**။
- `Building` လိုင်းရှိ ဂဏန်းအားလုံး (`100%`, `3/3`) — **bold + cyan**။
- `Compiled in` ဂဏန်း — **bold + cyan**။
- `Finished` — **bold + အစိမ်း**၊ bytes ဂဏန်း — **bold + cyan**။
- `Compiled in` သည် `Finished` အရှေ့ ရောက်လာသည်။
- Terminal မဟုတ်သော stream၊ `NO_COLOR=1` သို့မဟုတ် `TERM=dumb` တွင် escape code မပါဘဲ အလားတူ စာသားကိုပဲ ရသည် — script များ ဖတ်ရ လွယ်နေသည်။

## ၅။ ပြင်ဆင်ချက်များ

- `if` ပြီးနောက် လာသော statement သည် `if` body ထဲ ဝင်သွားနိုင်သည့် အမှားကို ပြင်ပြီး။ Function body ၏ အဆုံးကို `exit`/`return` ဖြင့် တိတိကျကျ ပိတ်လိုက်သဖြင့် `akk ir` တွင်လည်း မှန်ကန်စွာ ပြပြီး native backend ကလည်း မှန်ကန်စွာ လုပ်ဆောင်သည်။
- Windows PE output တွင် `.idata` ၏ RVA ကို text အရွယ်အစားမှ တွက်ယူသည်။ ယခင် ပုံသေ `0x2000` ကြောင့် text ကြီးလာလျှင် section ထပ်ဖြစ်ပြီး `Exec format error` ဖြစ်ခဲ့သည်။
- Native backend ၏ အတွင်းပိုင်း (SSE opcodes၊ byte/shift/lea encoders၊ `REX.X`) ကို ပြင်ဆင်ပြီး။

## ၆။ အတွင်းပိုင်း

- `akk ir [--opt] <file>` — middle end ၏ symbol table, HIR နှင့် MIR ကို ကြည့်နိုင်သည်။ `--opt` ဖြင့် `opt::const_fold` နှင့် `opt::dce` ပြီးနောက် MIR ကို ပြန်ပြသည်။
- Slice ကို `opt` မလုပ်မီ ဆုံးဖြတ်သဖြင့် optimiser က backend မထုတ်နိုင်သည့် construct တစ်ခုကို fold လုပ်ပြီး ဖြစ်ပေါ်စေတာ မရှိတော့ပါ။ `--opt` နှင့် `--no-opt` နှစ်မျိုးလုံး တူညီသော program များကို `E177` ဖြင့် ငြင်းသည်။

## ၇။ စမ်းသပ်မှု

- `cargo fmt --all -- --check` — clean။
- `cargo check --all-targets` — warning မရှိ။
- `cargo test --all-targets` — unit test ၂၁၀၊ `tests/cli.rs` ၇၊ `tests/cli_project.rs` ၁၃၊ `tests/bundle.rs` ၄၁၊ `tests/native/` ၁၃ အားလုံး အောင်မြင်။
- `tests/native/` သည် float case များကို `--opt` နှင့် `--no-opt` နှစ်မျိုးလုံးအတွက် binary တကယ် run ပြီး stdout နှင့် exit status ကို interpreter နှင့် byte-by-byte နှိုင်းယှဉ်သည်။

## ၈။ သိထားရန် ကန့်သတ်ချက်များ

- Native backend ၏ slice အပြင် construct များ (list/tuple/dict/set၊ class၊ closure) ကို `E177` ဖြင့် ဆက်လက် ငြင်းဆဲ ဖြစ်ပြီး ဖိုင်တစ်ခုမျှ မရေးပါ။
- int argument ကို float parameter သို့ အလိုအလျောက် promote လုပ်ခြင်း မရှိသေးပါ (`halve(3)` ကို `E177` ဖြင့် ငြင်းသည်၊ `halve(3.0)` ကို သုံးပါ)။
- Linux ELF output ကို ဤ host တွင် မ run နိုင်သေးပါ (`as`/`ld`/`cc` မရှိ)။ Windows `.exe` ကိုသာ တကယ် run စမ်းထားသည်။

---

## အဆင့်မြှင့်နည်း

```bash
akk update            # နောက်ဆုံး ဗားရှင်းသို့
akk update v3.0.0-alpha.3
akk --version         # akk 3.0.0-alpha.3
```

> **သတိ:** `akk run` သည် compiled ဖိုင်ကို `build/` ထဲ ထားသဖြင့် ယခင် ဗားရှင်းက source ဘေးတွင် ထားခဲ့သော `<file_name>.exe` ဖိုင်များကို မသုံးတော့ပါ။ လိုအပ်လျှင် ဖျက်နိုင်သည် — နောက် `akk run` တွင် `build/<file_name>.exe` အသစ် ပြန်လည် ဖန်တီးပေးမည်။
