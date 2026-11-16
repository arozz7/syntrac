//! Reading a Windows scheduled-task definition (the XML files under System32\Tasks): what it runs, when it starts, whether it is
//! switched off. Pure: text in, facts out. The sample mirrors the shape of real task files (a default namespace, an Exec action
//! with a Command and Arguments, a COM-handler action, triggers).

use syntrac_core::mini_xml::parse_xml;
use syntrac_core::task_xml::parse_task;

const TASK: &str = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>Example Corp</Author>
    <Description>Keeps &quot;things&quot; &amp; stuff up to date</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger><Enabled>true</Enabled></LogonTrigger>
    <BootTrigger/>
    <TimeTrigger><Enabled>false</Enabled><StartBoundary>2026-01-01T00:00:00</StartBoundary></TimeTrigger>
    <LogonTrigger><UserId>someone</UserId></LogonTrigger>
  </Triggers>
  <Settings>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>%ProgramFiles%\Example\updater.exe</Command>
      <Arguments>/check --quiet &amp; more</Arguments>
      <WorkingDirectory>C:\Temp</WorkingDirectory>
    </Exec>
    <Exec><Command>"C:\Tools\second.exe"</Command></Exec>
  </Actions>
</Task>"#;

#[test]
fn the_commands_arguments_triggers_and_enabled_state_are_read() {
    let task = parse_task(TASK).expect("a task");

    assert!(task.enabled);
    assert_eq!(task.triggers, ["boot", "logon", "time"], "once each, sorted, without the word Trigger");
    assert_eq!(task.actions.len(), 2);
    assert_eq!(task.actions[0].command, r"%ProgramFiles%\Example\updater.exe");
    assert_eq!(task.actions[0].arguments, "/check --quiet & more", "entities are decoded");
    assert_eq!(task.actions[1].command, r#""C:\Tools\second.exe""#);
    assert_eq!(task.actions[1].arguments, "");
    assert_eq!(task.com_handlers, 0);
}

#[test]
fn a_task_switched_off_in_its_settings_says_so_and_a_disabled_trigger_does_not_switch_it_off() {
    let off = TASK.replace("<Settings>\n    <Enabled>true</Enabled>", "<Settings>\n    <Enabled>false</Enabled>");

    assert!(!parse_task(&off).unwrap().enabled);
    assert!(parse_task(TASK).unwrap().enabled, "the TimeTrigger's own Enabled is not the task's");
}

#[test]
fn a_task_with_only_a_com_handler_has_no_command_to_map_and_is_counted() {
    let xml = r#"<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
      <Triggers><EventTrigger/><WnfStateChangeTrigger/></Triggers>
      <Actions Context="LocalSystem"><ComHandler><ClassId>{00000000-0000-0000-0000-000000000001}</ClassId></ComHandler></Actions>
    </Task>"#;

    let task = parse_task(xml).unwrap();

    assert!(task.actions.is_empty());
    assert_eq!(task.com_handlers, 1);
    assert_eq!(task.triggers, ["event", "wnfstatechange"]);
    assert!(task.enabled, "no Settings element: enabled by default");
}

#[test]
fn something_that_is_not_a_task_is_none_and_never_a_panic() {
    for text in ["", "not xml at all", "<Task><Actions></Task>", "<Other/>", "<Task", "<<<>>>"] {
        assert!(parse_task(text).is_none(), "{text:?}");
    }
}

#[test]
fn comments_cdata_and_character_references_are_handled() {
    let xml = "<Task><!-- a comment --><Actions><Exec><Command><![CDATA[C:\\a & b\\x.exe]]></Command><Arguments>&#65;&#x42;</Arguments></Exec></Actions></Task>";

    let task = parse_task(xml).unwrap();

    assert_eq!(task.actions[0].command, r"C:\a & b\x.exe");
    assert_eq!(task.actions[0].arguments, "AB");
}

#[test]
fn a_hostile_file_is_bounded_deeply_nested_or_huge_in_nodes() {
    let deep = format!("{}{}", "<a>".repeat(5_000), "</a>".repeat(5_000));
    assert!(parse_xml(&deep).is_none(), "nesting beyond the limit is refused");

    let wide = format!("<Task><Actions>{}</Actions></Task>", "<Exec><Command>x.exe</Command></Exec>".repeat(50_000));
    assert!(parse_xml(&wide).is_none(), "more nodes than the limit is refused");
}

#[test]
fn text_with_control_and_direction_characters_is_cleaned_when_read() {
    let xml = "<Task><Actions><Exec><Command>C:\\x\u{202E}.exe</Command><Arguments>a\u{0007}b</Arguments></Exec></Actions></Task>";

    let task = parse_task(xml).unwrap();

    assert_eq!(task.actions[0].command, r"C:\x.exe");
    assert_eq!(task.actions[0].arguments, "ab");
}

#[test]
fn the_mini_parser_gives_a_tree_with_the_text_of_each_element() {
    let root = parse_xml("<a x='1'><b>one</b><b>two</b><c/></a>").unwrap();

    assert_eq!(root.name, "a");
    assert_eq!(root.children_named("b").map(|n| n.text.as_str()).collect::<Vec<_>>(), ["one", "two"]);
    assert!(root.child("c").is_some() && root.child("missing").is_none());
    assert!(parse_xml("<a><b></a></b>").is_none(), "crossed tags are not accepted");
}
