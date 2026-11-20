pub(crate) fn missing_period_err(line: usize) -> String {
    format!("E002 လိုင်း {} ၏ စာကြောင်းအဆုံးတွင် '။' မရှိပါ။", line)
}

/// The eng (English-spelled) statement equivalent of `missing_period_err`: an
/// eng statement (`name :int = 5`, `print(...)`, `use ...`, ...) written
/// without its ';' terminator. Shares E002, which is the "statement
/// terminator is missing" code for every statement style.
pub(crate) fn missing_semicolon_err(line: usize) -> String {
    format!("E002 လိုင်း {} တွင် အဆုံးသတ် \';\' မရှိပါ။", line)
}

pub(crate) fn generic_syntax_err(line: usize) -> String {
    format!("E001 လိုင်း {} သည် ရေးသားပုံစည်းမျဉ်းမမှန်ကန်ပါ။", line)
}

pub(crate) fn unclosed_bracket_err(line: usize, open: char) -> String {
    let close = match open {
        '[' => ']',
        '(' => ')',
        '{' => '}',
        _ => '?',
    };
    format!("E003 လိုင်း {} တွင် '{}' ကိုပိတ်ရန် '{}' မရှိပါ။", line, open, close)
}

pub(crate) fn mismatched_bracket_err(line: usize) -> String {
    format!("E004 လိုင်း {} တွင် ကွင်းများ ({{}}/[]/()) မှန်ကန်စွာ မတွဲထားပါ။", line)
}

pub(crate) fn dict_entry_err(line: usize) -> String {
    format!(
        "E005 လိုင်း {} တွင် အဘိဓာန် အတွင်း <key> သည် <value> ဖြစ်၏။ ပုံစံဖြင့် ရေးရပါမည်။",
        line
    )
}

// Generic "function call" particle errors, shared by ဖော်ပြပါ / မေးပါ.
pub(crate) fn fn_missing_particle_err(line: usize, fname: &str) -> String {
    let full = format!("{}။", fname);
    format!(
        "E006 လိုင်း {}၌ \"{}\" လုပ်ဆောင်ရန် \"{}\" ၏အရှေ့၌ \"ကို\" ခံရေးရန်လိုအပ်သည်။",
        line, full, full
    )
}

pub(crate) fn fn_missing_value_err(line: usize, fname: &str) -> String {
    let full = format!("{}။", fname);
    format!(
        "E007 လိုင်း {}၌ \"{}\" လုပ်ဆောင်ရန် တန်ဖိုး မရှိသဖြင့် \"{}\" ကို မလုပ်ဆောင်နိုင်ပါ။",
        line, full, full
    )
}

/// Error for a line still using the removed `<value> ကို <type> သို့
/// ပြောင်းပါ။` sentence. When the type it asked for is recognizable, E008
/// names the conversion function that replaced it; otherwise E009 lists all
/// of them. E008/E009 were this sentence's own error codes before it was
/// removed, so old programs keep landing on the same code.
pub(crate) fn convert_removed_err(
    line: usize,
    target: Option<(&'static str, &'static str)>,
) -> String {
    match target {
        Some((mm, eng)) => format!(
            "E008 လိုင်း {} တွင် \"ပြောင်းပါ\" ကို ဖယ်ရှားလိုက်ပါပြီ။ <value> ကို {}(x) / {}(x) ဖြင့် ပြောင်းပါ။",
            line, mm, eng
        ),
        None => format!(
            "E009 လိုင်း {} တွင် \"ပြောင်းပါ\" ကို ဖယ်ရှားလိုက်ပါပြီ။ ပြောင်းရန်မှာ — ကိန်း(x)/ဂဏန်း(x), ကိန်းပြည့်(x)/int(x), ဒဿမ(x)/float(x), ဘူးလ်(x)/bool(x), စာ(x)/str(x)။",
            line
        ),
    }
}

pub(crate) fn math_missing_value_err(line: usize, fname: &str) -> String {
    format!(
        "E010 လိုင်း {} တွင် \"{}\" ကိုလုပ်ဆောင်ရန် တန်ဖိုး မရှိသဖြင့် \"{}\" ကိုမလုပ်ဆောင်နိုင်ပါ။",
        line, fname, fname
    )
}

pub(crate) fn if_missing_condition_err(line: usize) -> String {
    format!(
        "E011 လိုင်း {} တွင် \"အကယ်၍\"/\"သို့မဟုတ်\" အတွက် အခြေအနေ (စည်းကမ်းချက်) လိုအပ်ပါသည်။",
        line
    )
}

pub(crate) fn if_condition_syntax_err(line: usize) -> String {
    format!(
        "E012 လိုင်း {} တွင် အခြေအနေ ရေးသားပုံ မှားနေပါသည်။ အသုံးပြုပုံ — (<value> <တန်ဖိုးနှိုင်းယှဉ်ခြင်း> <value>) (နှင့်|သို့) (...) ဖြစ်လျှင်",
        line
    )
}

pub(crate) fn if_missing_then_err(line: usize) -> String {
    format!(
        "E013 လိုင်း {} တွင် အခြေအနေ အပြီးမှာ \"ဖြစ်လျှင်\" သို့မဟုတ် \"မဖြစ်လျှင်\" လိုအပ်ပါသည်။",
        line
    )
}

pub(crate) fn while_missing_condition_err(line: usize) -> String {
    format!(
        "E015 လိုင်း {} တွင် \"အခြေအနေ\" အတွက် အခြေအနေ (စည်းကမ်းချက်) လိုအပ်ပါသည်။",
        line
    )
}

pub(crate) fn while_condition_syntax_err(line: usize) -> String {
    format!(
        "E016 လိုင်း {} တွင် အခြေအနေ ရေးသားပုံ မှားနေပါသည်။ အသုံးပြုပုံ — အခြေအနေ (<value> <တန်ဖိုးနှိုင်းယှဉ်ခြင်း> <value>) (နှင့်|သို့) (...) ဖြစ်နေစဉ်",
        line
    )
}

pub(crate) fn while_missing_then_err(line: usize) -> String {
    format!(
        "E017 လိုင်း {} တွင် အခြေအနေ အပြီးမှာ \"ဖြစ်နေစဉ်\" / \"ဖြစ်နေရင်\" သို့မဟုတ် \"မဖြစ်နေစဉ်\" / \"မဖြစ်နေရင်\" လိုအပ်ပါသည်။",
        line
    )
}

pub(crate) fn func_missing_name_err(line: usize) -> String {
    format!(
        "E018 လိုင်း {} တွင် \"အလုပ်\" အတွက် အမည် လိုအပ်ပါသည်။ အသုံးပြုပုံ — အလုပ် <fn name> အတွက် <parameter> ဖြင့်",
        line
    )
}

pub(crate) fn func_bad_param_err(line: usize) -> String {
    format!(
        "E019 လိုင်း {} တွင် \"အလုပ်\" ၏ အချက် ရေးသားပုံ မှားနေပါသည်။ အသုံးပြုပုံ — အလုပ် <fn name> အတွက် <parameter> ဖြင့်",
        line
    )
}

pub(crate) fn func_missing_by_err(line: usize) -> String {
    format!(
        "E020 လိုင်း {} တွင် \"အလုပ်\" ၏ ခေါင်းစီး အပြီးမှာ \"ဖြင့်\" (အချက် ပါလျှင်) သို့မဟုတ် \"သည်\" (အချက် မပါလျှင်) လိုအပ်ပါသည်။",
        line
    )
}

pub(crate) fn func_call_bad_name_err(line: usize) -> String {
    format!("E021 လိုင်း {} တွင် ခေါ်လိုသော အလုပ်၏ အမည် မှားနေပါသည်။", line)
}

pub(crate) fn func_call_missing_particle_err(line: usize) -> String {
    format!(
        "E022 လိုင်း {} တွင် အလုပ် ခေါ်ရန် \"<fn name> ကို လုပ်ပါ။\" သို့မဟုတ် \"<fn name> ကို လုပ်ရန် <argument> ဖြင့်\" ပုံစံဖြင့် ရေးရပါမည်။",
        line
    )
}

pub(crate) fn func_call_missing_by_err(line: usize) -> String {
    format!(
        "E023 လိုင်း {} တွင် \"လုပ်ရန်\" ၏ အချက် အပြီးမှာ \"ဖြင့်\" လိုအပ်ပါသည်။",
        line
    )
}

pub(crate) fn func_call_missing_arg_err(line: usize) -> String {
    format!("E024 လိုင်း {} တွင် \"လုပ်ရန်\" ရန် အချက် လိုအပ်ပါသည်။", line)
}

pub(crate) fn library_use_bad_list_err(line: usize) -> String {
    format!(
        "E072 လိုင်း {} တွင် \"နည်းပညာများ\" ၏ နည်းပညာ အမည်များကို \",\" ဖြင့် ခွဲ၍ရေးပါ။ အသုံးပြုပုံ — နည်းပညာများ <lib1>, <lib2> ကို သုံးမည်။",
        line
    )
}

pub(crate) fn library_use_alias_err(line: usize) -> String {
    format!(
        "E093 လိုင်း {} တွင် \"အဖြစ်\" ၏ နောက်တွင် အမည်လွှဲ အမည် တစ်ခု လိုအပ်ပါသည်။ အသုံးပြုပုံ — နည်းပညာများ <lib> အဖြစ် <alias> ကို သုံးမည်။",
        line
    )
}

/// `နည်းပညာများ <lib> သုံးမည်။` -- the statement wrote "သုံးမည်" without
/// the "ကို" particle in front of it.
pub(crate) fn library_use_missing_particle_err(line: usize, lib_name: &str) -> String {
    format!(
        "E094 လိုင်း {} တွင် နည်းပညာ({}) ကို အသုံးပြုရန် \"ကို\" ခံရေးရန် လိုအပ်ပါသည်",
        line, lib_name
    )
}

pub(crate) fn func_alias_missing_name_err(line: usize) -> String {
    format!(
        "E075 လိုင်း {} တွင် \"အဖြစ်\" ၏ ရှေ့တွင် အမည်လွှဲ ပေးလိုသော အလုပ် အမည် တစ်ခုတည်း လိုအပ်ပါသည်။ အသုံးပြုပုံ — <fn name> အဖြစ် <alias1>, <alias2>",
        line
    )
}

pub(crate) fn func_alias_missing_alias_err(line: usize) -> String {
    format!(
        "E076 လိုင်း {} တွင် \"အဖြစ်\" ၏ နောက်တွင် အမည်လွှဲ အမည် အနည်းဆုံး တစ်ခု လိုအပ်ပါသည်။ အသုံးပြုပုံ — <fn name> အဖြစ် <alias1>, <alias2>",
        line
    )
}

pub(crate) fn func_alias_bad_alias_err(line: usize) -> String {
    format!(
        "E077 လိုင်း {} တွင် အမည်လွှဲ အမည်များကို \",\" ဖြင့် ခွဲ၍ တစ်ခုချင်း ရေးပါ။ အသုံးပြုပုံ — <fn name> အဖြစ် <alias1>, <alias2>",
        line
    )
}

pub(crate) fn func_call_assign_missing_var_err(line: usize) -> String {
    format!(
        "E068 လိုင်း {} တွင် အလုပ် ခေါ်ပြီး တန်ဖိုးသိမ်းမည့် ကိန်းရှင် အမည် လိုအပ်ပါသည်။ အသုံးပြုပုံ — <var> အတွက် <fn name> ကို လုပ်ပါ။ သို့မဟုတ် <var> အတွက် <fn name> ကို လုပ်ရန် <argument> ဖြင့်",
        line
    )
}

pub(crate) fn func_call_assign_bad_name_err(line: usize) -> String {
    format!("E069 လိုင်း {} တွင် ခေါ်လိုသော အလုပ်၏ အမည် မှားနေပါသည်။", line)
}

pub(crate) fn func_call_assign_missing_particle_err(line: usize) -> String {
    format!(
        "E070 လိုင်း {} တွင် အလုပ် ခေါ်ရန် \"<var> အတွက် <fn name> ကို လုပ်ပါ။\" သို့မဟုတ် \"<var> အတွက် <fn name> ကို လုပ်ရန် <argument> ဖြင့်\" ပုံစံဖြင့် ရေးရပါမည်။",
        line
    )
}

pub(crate) fn lib_call_syntax_err(line: usize) -> String {
    format!(
        "E084 လိုင်း {} တွင် နည်းပညာ အလုပ် ခေါ်ရန် ရေးသားပုံ မှားနေပါသည်။ အသုံးပြုပုံ — <lib> ၏ <fn>(<argument>) ကို လုပ်ပါ။ သို့မဟုတ် <lib> ၏ <fn> ကို လုပ်ရန် <argument> ဖြင့်",
        line
    )
}

pub(crate) fn class_missing_name_err(line: usize) -> String {
    format!(
        "E025 လိုင်း {} တွင် \"ပုံသေ\" အတွက် အမည် လိုအပ်ပါသည်။ အသုံးပြုပုံ — ပုံသေ <class name>။",
        line
    )
}

pub(crate) fn class_missing_period_err(line: usize) -> String {
    format!(
        "E026 လိုင်း {} တွင် \"ပုံသေ <class name>\" ၏ အပြီးမှာ \"သည်\" သို့မဟုတ် '။' လိုအပ်ပါသည်။",
        line
    )
}

pub(crate) fn class_body_not_method_err(line: usize) -> String {
    format!(
        "E027 လိုင်း {} တွင် \"ပုံသေ\" အတွင်း \"အလုပ်\" များသာ ပါဝင်နိုင်ပါသည်။",
        line
    )
}

pub(crate) fn class_bad_param_err(line: usize) -> String {
    format!(
        "E095 လိုင်း {} တွင် \"ပုံသေ\" ၏ တည်ဆောက်သူ အချက် ရေးသားပုံ မှားနေပါသည်။ အသုံးပြုပုံ — ပုံသေ <class name>(<parameter>, ...) သည်",
        line
    )
}

pub(crate) fn class_missing_constructor_err(line: usize, name: &str) -> String {
    format!(
        "E028 လိုင်း {} တွင် \"{}\" ပုံသေသည် တည်ဆောက်သူ အနေဖြင့် လုပ်ဆောင်ချက် အနည်းဆုံး တစ်ခု လိုအပ်ပါသည်။",
        line, name
    )
}

pub(crate) fn new_obj_missing_class_err(line: usize) -> String {
    format!(
        "E029 လိုင်း {} တွင် အရာဝတ္ထု ဖန်တီးသည့် ပုံသေ ရေးပုံ မှားနေပါသည်။ \"အသစ်\" မလိုအပ်တော့ပါ။ အသုံးပြုပုံ — <class name> (<argument>, ...)",
        line
    )
}

/// A ternary with its `?` but without the `:` and the else branch.
pub(crate) fn ternary_bad_form_err(line: usize) -> String {
    format!(
        "E151 လိုင်း {} တွင် `<condition> ? <then> : <else>` ကို မျှော်လင့်ပါသည် ('?' နှင့် ':' နှစ်ခုလုံး လိုအပ်ပါသည်)။",
        line
    )
}

pub(crate) fn eng_bad_form_err(line: usize) -> String {
    format!(
        "E100 လိုင်း {} တွင် `<name> :<type> = <value>;` သို့မဟုတ် `pin <name> :<type> = <value>;` ကို မျှော်လင့်ပါသည် (အမျိုးအစားများ — int, float, str, bool)။",
        line
    )
}

pub(crate) fn eng_bad_type_err(line: usize, type_name: &str) -> String {
    format!(
        "E101 လိုင်း {} တွင် \"{}\" ဆိုသော အမျိုးအစားကို မသိပါ (အမျိုးအစားများ — int, float, str, bool)။",
        line, type_name
    )
}

pub(crate) fn eng_missing_value_err(line: usize) -> String {
    format!("E102 လိုင်း {} တွင် '=' နောက်၌ တန်ဖိုး မပါပါ။", line)
}

pub(crate) fn eng_call_bad_form_err(line: usize, fn_name: &str) -> String {
    format!("E106 လိုင်း {} တွင် `{}(...);` ကို မျှော်လင့်ပါသည်။", line, fn_name)
}

pub(crate) fn eng_func_alias_bad_form_err(line: usize) -> String {
    format!(
        "E137 လိုင်း {} တွင် `function() as alias;` သို့မဟုတ် `function as alias;` ကို မျှော်လင့်ပါသည်။",
        line
    )
}

pub(crate) fn eng_block_form_err(line: usize) -> String {
    format!(
        "E109 လိုင်း {} တွင် `if (<cond>) {{ ... }}` (လိုအပ်လျှင် `else if`/`else` ဖြင့်)၊ `while (<cond>) {{ ... }}`၊ `for (<init>; <cond>; <step>) {{ ... }}`၊ `loop {{ ... }}`၊ `switch (<value>) {{ case <value>: ... default: ... }}` သို့မဟုတ် `fn <name>(<params>) {{ ... }}` ကို မျှော်လင့်ပါသည်။",
        line
    )
}

/// A `fn` written as a value but not in the lambda form
/// `fn(<params>) { <body> }`.
pub(crate) fn lambda_bad_form_err(line: usize) -> String {
    format!(
        "E150 လိုင်း {} တွင် `fn(<params>) {{ <body> }}` ပုံစံ lambda ကို မျှော်လင့်ပါသည်။",
        line
    )
}

pub(crate) fn eng_for_form_err(line: usize) -> String {
    format!(
        "E109 လိုင်း {} တွင် `for (<init>; <cond>; <step>) {{ ... }}` ကို မျှော်လင့်ပါသည်။ ဥပမာ — `for (i = 0; i < 10; i = i + 1) {{ ... }}`",
        line
    )
}

pub(crate) fn eng_for_in_form_err(line: usize) -> String {
    format!(
        "E109 လိုင်း {} တွင် `for <name> in <value> {{ ... }}` ကို မျှော်လင့်ပါသည်။ ဥပမာ — `for x in xs {{ ... }}`",
        line
    )
}

pub(crate) fn eng_return_bad_form_err(line: usize) -> String {
    format!(
        "E109 လိုင်း {} တွင် `return;` သို့မဟုတ် `return <value>;` ကို မျှော်လင့်ပါသည်။",
        line
    )
}

pub(crate) fn eng_switch_form_err(line: usize) -> String {
    format!(
        "E153 လိုင်း {} တွင် `switch (<value>) {{ case <value>: ... default: ... }}` ကို မျှော်လင့်ပါသည်။",
        line
    )
}

pub(crate) fn myanmar_switch_form_err(line: usize) -> String {
    format!(
        "E156 လိုင်း {} တွင် \"ရွေးရန် (<တန်ဖိုး>) ... ပြီး။\" ပုံစံဖြင့် ရေးပါ (အကိုင်းတစ်ခုစီသည် \"<တန်ဖိုး> ဖြစ်ရင် ...\" သို့မဟုတ် \"မူလ ...\")။",
        line
    )
}

pub(crate) fn myanmar_ternary_bad_form_err(line: usize) -> String {
    format!(
        "E157 လိုင်း {} တွင် \"(<အခြေအနေ>) ဖြစ်ရင် <တန်ဖိုး> မဟုတ်ရင် <တန်ဖိုး>\" ပုံစံဖြင့် ရေးပါ။",
        line
    )
}

pub(crate) fn myanmar_destructure_bad_form_err(line: usize) -> String {
    format!(
        "E158 လိုင်း {} တွင် \"[a, b] သည် <စာရင်း> ဖြစ်၏။\" ပုံစံဖြင့် နာမည်များသာ ရေးပါ။",
        line
    )
}

pub(crate) fn eng_use_bad_form_err(line: usize) -> String {
    format!(
        "E118 လိုင်း {} တွင် `use <library> [as <alias>] [, ...];` ကို မျှော်လင့်ပါသည်။",
        line
    )
}

pub(crate) fn destructure_bad_form_err(line: usize) -> String {
    format!(
        "E152 လိုင်း {} တွင် `[a, b] = <list>` ပုံစံ ခွဲထုတ်သတ်မှတ်မှုကို မျှော်လင့်ပါသည် (အမည်များသာ၊ ကော်မာဖြင့် ခွဲပါ)။",
        line
    )
}
