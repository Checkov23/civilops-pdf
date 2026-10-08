use super::*;

fn fields() -> Vec<FieldState> {
    let mut agree = FieldState::new("agree", FieldType::CheckBox, vec![]);
    agree.options = vec![("Yes".into(), "Yes".into())];
    let mut size = FieldState::new("size", FieldType::ComboBox, vec!["m".into()]);
    size.options = vec![("s".into(), "Small".into()), ("m".into(), "Medium".into()), ("l".into(), "Large".into())];
    vec![
        FieldState::new("price", FieldType::Text, vec!["12.5".into()]),
        FieldState::new("qty", FieldType::Text, vec!["4".into()]),
        FieldState::new("total", FieldType::Text, vec![]),
        FieldState::new("zip", FieldType::Text, vec!["02134".into()]),
        agree,
        size,
    ]
}

fn doc() -> DocInfo {
    DocInfo { file_name: "order.pdf".into(), num_pages: 3, page: 0, info: vec![("title".into(), "Order".into())] }
}

fn go(script: &str, event: &Event) -> Outcome {
    run(script, event, &doc(), &fields(), &[], Limits::default())
}

#[test]
fn calculate_scripts_read_fields_and_set_the_value() {
    let o = go("event.value = this.getField('price').value * getField('qty').value;", &Event::field("Calculate", "total", ""));
    assert_eq!(o.error, None);
    assert_eq!(o.value, "50");
    assert!(o.rc);
}

#[test]
fn numbers_with_leading_zeros_stay_text() {
    let o = go("event.value = typeof getField('zip').value + ':' + typeof getField('qty').value;", &Event::field("Calculate", "total", ""));
    assert_eq!(o.value, "string:number");
}

#[test]
fn validate_scripts_reject_with_rc_and_alert() {
    let o = go("if (event.value > 10) { app.alert('Too many'); event.rc = false; }", &Event::field("Validate", "qty", "11"));
    assert!(!o.rc);
    assert_eq!(o.alerts, ["Too many"]);
    let o = go("if (event.value > 10) { event.rc = false; }", &Event::field("Validate", "qty", "3"));
    assert!(o.rc);
}

#[test]
fn keystroke_scripts_see_the_change() {
    let mut e = Event::field("Keystroke", "qty", "1");
    e.change = "x".into();
    e.will_commit = false;
    let o = go("event.rc = /^[0-9]*$/.test(event.change);", &e);
    assert!(!o.rc);
}

#[test]
fn scripts_change_other_fields_and_their_properties() {
    let o = go(
        "var t = getField('total'); t.value = 99; t.readonly = true; t.display = display.hidden; \
         getField('agree').checkThisBox(0, true); getField('size').value = 'l'; t.textColor = color.red;",
        &Event::field("Mouse Up", "agree", ""),
    );
    assert_eq!(o.error, None);
    let total = o.changed.iter().find(|f| f.name == "total").unwrap();
    assert_eq!(total.value, ["99"]);
    assert!(total.readonly);
    assert_eq!(total.display, DISPLAY_HIDDEN);
    assert_eq!(total.text_color.as_deref(), Some(&["RGB".to_string(), "1".into(), "0".into(), "0".into()][..]));
    assert_eq!(o.changed.iter().find(|f| f.name == "agree").unwrap().value, ["Yes"]);
    assert_eq!(o.changed.iter().find(|f| f.name == "size").unwrap().value, ["l"]);
}

#[test]
fn choice_and_check_box_object_model() {
    let o = go(
        "var s = getField('size'); event.value = [s.numItems, s.getItemAt(2, false), s.currentValueIndices, s.valueAsString, \
         getField('agree').value, getField('agree').isBoxChecked(0), s.type].join('|');",
        &Event::field("Calculate", "total", ""),
    );
    assert_eq!(o.value, "3|Large|1|m|Off|false|combobox");
}

#[test]
fn util_printf_printd_and_printx() {
    let o = go(
        "event.value = [util.printf('%,0.2f', 1234567.891), util.printf('%05d|%s|%x', 42, 'hi', 255), util.printf('%,2.2f', 1234.5), \
         util.printd('mmm d, yyyy HH:MM', new Date(2024, 0, 5, 9, 7)), util.printd('dddd', new Date(2024, 0, 5)), \
         util.printx('(999) 999-9999', '5551234567'), util.printx('>AAA', 'abc')].join('|');",
        &Event::field("Calculate", "total", ""),
    );
    assert_eq!(o.error, None);
    assert_eq!(o.value, "1,234,567.89|00042|hi|FF|1.234,50|Jan 5, 2024 09:07|Friday|(555) 123-4567|ABC");
}

#[test]
fn document_requests_and_console() {
    let o = go(
        "console.println('hello ' + this.documentFileName + ' ' + numPages + ' ' + info.title); this.pageNum = 2; \
         this.resetForm(['qty']); this.print(); app.launchURL('https://example.org'); this.submitForm({cURL: 'https://example.org/f'});",
        &Event::field("Mouse Up", "agree", ""),
    );
    assert_eq!(o.error, None);
    assert_eq!(o.console, ["hello order.pdf 3 Order"]);
    assert_eq!(
        o.requests,
        [
            Request::GoToPage(2),
            Request::Reset(vec!["qty".into()]),
            Request::Print,
            Request::LaunchUrl("https://example.org".into()),
            Request::Submit("https://example.org/f".into())
        ]
    );
}

#[test]
fn document_level_functions_are_available() {
    let o = run(
        "event.value = double(getField('qty').value);",
        &Event::field("Calculate", "total", ""),
        &doc(),
        &fields(),
        &["function double(x) { return x * 2; }".into()],
        Limits::default(),
    );
    assert_eq!(o.value, "8");
}

#[test]
fn errors_and_runaway_scripts_are_contained() {
    let o = go("event.value = nosuch.thing;", &Event::field("Calculate", "total", "keep"));
    assert!(o.error.is_some());
    let o = run("while (true) {}", &Event::doc("Open"), &doc(), &fields(), &[], Limits { loop_iterations: 10_000, recursion: 64 });
    assert!(o.error.is_some(), "the loop limit stops it");
    let o = go("function f() { return f(); } f();", &Event::doc("Open"));
    assert!(o.error.is_some(), "the recursion limit stops it");
    let o = go("getField('nope').value", &Event::doc("Open"));
    assert!(o.error.as_deref().unwrap_or("").contains("null") || o.error.is_some());
}

#[test]
fn the_sandbox_has_no_host_access() {
    let o = go(
        "event.value = [typeof require, typeof process, typeof fetch, typeof XMLHttpRequest, typeof importScripts].join(',');",
        &Event::doc("Open"),
    );
    assert_eq!(o.value, "undefined,undefined,undefined,undefined,undefined");
}

/// Hostile nesting used to overflow the stack in boa's parser and compiler, aborting the app
/// (about 100 nested parentheses were enough on a 2 MiB stack).
#[test]
fn deeply_nested_scripts_fail_instead_of_crashing() {
    let n = 100_000;
    let hostile = [
        format!("event.value = {}1{};", "(".repeat(n), ")".repeat(n)),
        format!("var a = {}1{};", "[".repeat(n), "]".repeat(n)),
        format!("{}{}", "{".repeat(n), "}".repeat(n)),
        format!("var a = {}1;", "!".repeat(n)),
        format!("var a = {}1;", "- ".repeat(n)),
        format!("var a = {}1;", "1?1:".repeat(n)),
        format!("var f = {}1;", "a=>".repeat(n)),
    ];
    for s in &hostile {
        let o = go(s, &Event::doc("Open"));
        assert!(o.error.is_some(), "{}…", &s[..20]);
    }
    // A hostile document-level script is refused too, before the field script runs.
    let o = run("event.value = 'ran';", &Event::field("Calculate", "a", ""), &doc(), &fields(), &[hostile[0].clone()], Limits::default());
    assert!(o.error.is_some());
}

#[test]
fn ordinary_nesting_still_runs() {
    let n = 40;
    let s = format!("event.value = {}1{} + [[[2]]][0][0][0] + (1 ? 2 : 3);", "(".repeat(n), ")".repeat(n));
    let o = go(&s, &Event::field("Calculate", "a", ""));
    assert_eq!(o.error, None);
    assert_eq!(o.value, "5");
    // Brackets in strings and comments don't count.
    let s = format!("// {}\nevent.value = '{}'.length;", "(".repeat(1000), "[".repeat(1000));
    assert_eq!(go(&s, &Event::field("Calculate", "a", "")).value, "1000");
}
