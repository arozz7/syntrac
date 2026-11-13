use syntrac_app_lib::names::display_name;

#[test]
fn a_path_shows_just_the_file_name() {
    assert_eq!(display_name(r"C:\Program Files\Google\Chrome\Application\chrome.exe", 10), "chrome.exe");
    assert_eq!(display_name("C:/tools/jq.exe", 10), "jq.exe", "forward slashes work too");
}

#[test]
fn the_system_and_idle_processes_have_no_path_but_get_their_real_names() {
    assert_eq!(display_name("", 4), "System");
    assert_eq!(display_name("", 0), "Idle");
}

#[test]
fn an_unknown_process_with_no_path_is_identified_by_its_pid() {
    assert_eq!(display_name("", 1234), "pid 1234");
}

#[test]
fn odd_paths_never_produce_an_empty_name() {
    assert_eq!(display_name(r"C:\tools\", 7), "pid 7");
    assert_eq!(display_name("   ", 7), "pid 7");
}
