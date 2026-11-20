//! Tests that run whole scripts through the engine.

use std::time::Duration;

use crate::{Host, Interpreter, XtError};

#[derive(Default)]
struct Capture {
    out: Vec<String>,
    slept: Vec<Duration>,
}

impl Host for Capture {
    fn message(&mut self, text: &str) {
        self.out.push(text.to_string());
    }

    // Record waits instead of really sleeping, so tests stay fast.
    fn sleep(&mut self, duration: Duration) {
        self.slept.push(duration);
    }
}

fn run_full(src: &str) -> Capture {
    let mut interp = Interpreter::new();
    let mut host = Capture::default();
    interp
        .run_source(src, &mut host)
        .expect("script should run");
    host
}

fn run(src: &str) -> Vec<String> {
    run_full(src).out
}

fn run_err(src: &str) -> XtError {
    let mut interp = Interpreter::new();
    let mut host = Capture::default();
    interp
        .run_source(src, &mut host)
        .expect_err("script should fail")
}

// ----- step 1 behavior still works -------------------------------------------

#[test]
fn hello_world() {
    assert_eq!(run("put \"Hello World\""), vec!["Hello World"]);
}

#[test]
fn concatenation_and_arithmetic() {
    assert_eq!(run("put \"a\" & \"b\""), vec!["ab"]);
    assert_eq!(run("put \"a\" && \"b\""), vec!["a b"]);
    assert_eq!(run("put 2 + 3 * 4"), vec!["14"]);
    assert_eq!(run("put (2 + 3) * 4"), vec!["20"]);
    assert_eq!(run("put -5 + 2"), vec!["-3"]);
    assert_eq!(run("put 2 - -3"), vec!["5"]);
    assert_eq!(run("put 10 / 4"), vec!["2.5"]);
    assert_eq!(run("put 7 div 2"), vec!["3"]);
    assert_eq!(run("put 7 mod 2"), vec!["1"]);
}

#[test]
fn variables() {
    assert_eq!(run("PUT \"Hi\" INTO Greeting\nput greeting"), vec!["Hi"]);
    assert_eq!(run("put banana"), vec!["banana"]);
    assert_eq!(
        run("put \"b\" into x\nput \"a\" before x\nput \"c\" after x\nput x"),
        vec!["abc"]
    );
    assert_eq!(run("get 2 * 21\nput it"), vec!["42"]);
}

#[test]
fn no_reserved_words() {
    assert_eq!(run("put 3 into put\nput put"), vec!["3"]);
}

#[test]
fn comments_blank_lines_and_continuation() {
    assert_eq!(run("\n-- a comment\nput \"x\" -- trailing"), vec!["x"]);
    assert_eq!(run("put \"a\" & ¬\n  \"b\""), vec!["ab"]);
}

// ----- comparisons and logic -------------------------------------------------

#[test]
fn comparisons_and_logic() {
    assert_eq!(run("put 10 > 9"), vec!["true"]);
    assert_eq!(run("put \"apple\" < \"banana\""), vec!["true"]);
    assert_eq!(run("put 1 = 1.0"), vec!["true"]);
    assert_eq!(run("put \"abc\" is \"ABC\""), vec!["true"]);
    assert_eq!(run("put 1 is not 2"), vec!["true"]);
    assert_eq!(run("put 1 <> 1"), vec!["false"]);
    assert_eq!(run("put \"abc\" contains \"B\""), vec!["true"]);
    assert_eq!(run("put \"b\" is in \"abc\""), vec!["true"]);
    assert_eq!(run("put \"z\" is not in \"abc\""), vec!["true"]);
    assert_eq!(run("put not (1 = 2)"), vec!["true"]);
    assert_eq!(run("put 1 < 2 and 2 < 3"), vec!["true"]);
    assert_eq!(run("put 1 > 2 or 2 < 3"), vec!["true"]);
}

// ----- if --------------------------------------------------------------------

#[test]
fn one_line_if() {
    assert_eq!(run("if 1 < 2 then put \"yes\" else put \"no\""), vec!["yes"]);
    assert_eq!(run("if 1 > 2 then put \"yes\" else put \"no\""), vec!["no"]);
    assert_eq!(run("if 1 > 2 then put \"yes\"\nput \"after\""), vec!["after"]);
}

#[test]
fn block_if_with_else_if_chain() {
    let script = r#"
put 5 into x
if x < 3 then
  put "small"
else if x < 10 then
  put "medium"
else
  put "large"
end if
"#;
    assert_eq!(run(script), vec!["medium"]);
}

// ----- repeat ----------------------------------------------------------------

#[test]
fn repeat_times() {
    assert_eq!(run("repeat 3 times\n  put \"hi\"\nend repeat"), vec!["hi", "hi", "hi"]);
    assert_eq!(run("repeat for 2 times\n  put \"x\"\nend repeat"), vec!["x", "x"]);
}

#[test]
fn repeat_with() {
    assert_eq!(
        run("repeat with i = 1 to 3\n  put i\nend repeat"),
        vec!["1", "2", "3"]
    );
    assert_eq!(
        run("repeat with i = 3 down to 1\n  put i\nend repeat"),
        vec!["3", "2", "1"]
    );
}

#[test]
fn repeat_while_and_until() {
    let while_loop = "put 0 into n\nrepeat while n < 3\n  put n\n  put n + 1 into n\nend repeat";
    assert_eq!(run(while_loop), vec!["0", "1", "2"]);
    let until_loop = "put 0 into n\nrepeat until n = 3\n  put n\n  put n + 1 into n\nend repeat";
    assert_eq!(run(until_loop), vec!["0", "1", "2"]);
}

#[test]
fn exit_and_next_repeat() {
    let script = r#"
repeat with i = 1 to 5
  if i = 2 then next repeat
  if i = 4 then exit repeat
  put i
end repeat
"#;
    assert_eq!(run(script), vec!["1", "3"]);

    let counted = r#"
put 0 into n
repeat 5 times
  put n + 1 into n
  if n = 2 then next repeat
  put n
end repeat
"#;
    assert_eq!(run(counted), vec!["1", "3", "4", "5"]);
}

#[test]
fn exit_repeat_leaves_only_the_inner_loop() {
    let script = r#"
repeat with i = 1 to 2
  repeat with j = 1 to 3
    if j = 2 then exit repeat
    put i & j
  end repeat
end repeat
"#;
    assert_eq!(run(script), vec!["11", "21"]);
}

#[test]
fn forever_with_exit() {
    let script = r#"
put 0 into n
repeat forever
  put n + 1 into n
  if n = 3 then exit repeat
end repeat
put n
"#;
    assert_eq!(run(script), vec!["3"]);
}

// ----- handlers --------------------------------------------------------------

#[test]
fn command_handler() {
    let script = r#"
on greet name
  put "Hello, " & name & "!"
end greet

greet "World"
"#;
    assert_eq!(run(script), vec!["Hello, World!"]);
}

#[test]
fn handlers_can_be_defined_after_use() {
    let script = "greet\non greet\n  put \"hi\"\nend greet";
    assert_eq!(run(script), vec!["hi"]);
}

#[test]
fn function_handler_and_recursion() {
    let script = r#"
function double n
  return n * 2
end double

function fact n
  if n <= 1 then return 1
  return n * fact(n - 1)
end fact

put double(21)
put fact(5)
"#;
    assert_eq!(run(script), vec!["42", "120"]);
}

#[test]
fn the_result_holds_what_a_command_returned() {
    let script = "on setx\n  return \"done\"\nend setx\nsetx\nput the result";
    assert_eq!(run(script), vec!["done"]);
}

#[test]
fn both_argument_styles() {
    let script = r#"
on show a b
  put a && b
end show

show "x" "y"
show 1, 2
show (1 + 1) "go"
"#;
    assert_eq!(run(script), vec!["x y", "1 2", "2 go"]);
}

#[test]
fn parameters_may_be_separated_by_commas_or_spaces() {
    let script = "on show a b, c\n  put a & b & c\nend show\nshow 1 2 3";
    assert_eq!(run(script), vec!["123"]);
}

#[test]
fn missing_arguments_are_empty() {
    assert_eq!(run("on show a b\n  put a && b\nend show\nshow \"x\""), vec!["x "]);
}

#[test]
fn globals_are_shared_between_handlers() {
    let script = r#"
on bump
  global counter
  put counter + 1 into counter
end bump

on start
  global counter
  put 0 into counter
  bump
  bump
  put counter
end start

start
"#;
    assert_eq!(run(script), vec!["2"]);
}

#[test]
fn exit_to_top_stops_everything() {
    let script = r#"
on inner
  exit to top
  put "not reached"
end inner

on outer
  inner
  put "not reached either"
end outer

outer
put "not reached at all"
"#;
    assert!(run(script).is_empty());
}

#[test]
fn exit_handler_leaves_only_that_handler() {
    let script = "on early\n  put \"a\"\n  exit early\n  put \"b\"\nend early\nearly\nput \"c\"";
    assert_eq!(run(script), vec!["a", "c"]);
}

#[test]
fn runaway_recursion_is_an_error_not_a_crash() {
    let err = run_err("on loopy\n  loopy\nend loopy\nloopy");
    assert!(err.message.contains("Too many"));
}

// ----- wait ------------------------------------------------------------------

#[test]
fn wait_asks_the_host_to_sleep() {
    let host = run_full("wait 2 seconds\nwait 30 ticks\nwait 1");
    assert_eq!(
        host.slept,
        vec![
            Duration::from_secs(2),
            Duration::from_millis(500),
            Duration::from_secs_f64(1.0 / 60.0),
        ]
    );
}

#[test]
fn wait_inside_a_function_called_from_an_expression() {
    let script = r#"
function slow n
  wait 1 second
  return n + 1
end slow

put slow(1) + 10
"#;
    let host = run_full(script);
    assert_eq!(host.out, vec!["12"]);
    assert_eq!(host.slept, vec![Duration::from_secs(1)]);
}

#[test]
fn wait_in_a_loop() {
    let host = run_full("repeat 3 times\n  wait 1 tick\nend repeat");
    assert_eq!(host.slept.len(), 3);
}

#[test]
fn wait_until_and_while_with_conditions_already_decided() {
    let host = run_full("wait until 1 = 1\nwait while 1 = 2\nput \"done\"");
    assert_eq!(host.out, vec!["done"]);
    assert!(host.slept.is_empty());
}

#[test]
fn run_returns_sleeping_instead_of_blocking() {
    use crate::Status;
    let mut interp = Interpreter::new();
    let mut host = Capture::default();
    interp
        .load("put \"before\"\nwait 5 seconds\nput \"after\"")
        .unwrap();
    let status = interp.run(&mut host).unwrap();
    assert_eq!(status, Status::Sleeping(Duration::from_secs(5)));
    assert_eq!(host.out, vec!["before"]);
    // The host decides when to resume.
    assert_eq!(interp.run(&mut host).unwrap(), Status::Finished);
    assert_eq!(host.out, vec!["before", "after"]);
}

// ----- the prompt's memory ---------------------------------------------------

#[test]
fn variables_and_handlers_persist_between_loads() {
    let mut interp = Interpreter::new();
    let mut host = Capture::default();
    interp.run_source("put 5 into x", &mut host).unwrap();
    interp
        .run_source("on addone n\n  return n + 1\nend addone", &mut host)
        .unwrap();
    interp.run_source("put addone(x)", &mut host).unwrap();
    assert_eq!(host.out, vec!["6"]);
}

#[test]
fn variables_survive_an_error() {
    let mut interp = Interpreter::new();
    let mut host = Capture::default();
    interp.run_source("put 5 into x", &mut host).unwrap();
    assert!(interp.run_source("put 1 / 0", &mut host).is_err());
    interp.run_source("put x", &mut host).unwrap();
    assert_eq!(host.out, vec!["5"]);
}

// ----- errors ----------------------------------------------------------------

#[test]
fn runtime_errors_carry_a_line_number() {
    let err = run_err("put 1\nput 1 / 0");
    assert_eq!(err.message, "Can't divide by zero");
    assert_eq!(err.line, Some(1));
    assert!(run_err("put \"abc\" + 1").message.contains("Expected a number"));
    assert!(run_err("if 5 then put 1").message.contains("true or false"));
}

#[test]
fn unknown_handler() {
    assert_eq!(run_err("nosuch").message, "Can't understand \"nosuch\"");
}

#[test]
fn syntax_errors_have_a_position() {
    let err = run_err("put");
    assert_eq!((err.line, err.col), (Some(0), Some(3)));
    let err = run_err("put \"a\" into");
    assert_eq!((err.line, err.col), (Some(0), Some(12)));
    let err = run_err("put 1\nput \"oops");
    assert_eq!((err.line, err.col), (Some(1), Some(4)));
    assert!(err.message.contains("closing quote"));
}

#[test]
fn mixing_argument_styles_is_an_error() {
    let script = "on show a b\n  put a\nend show\nshow 1 + 1 \"go\"";
    assert!(run_err(script).message.contains("parentheses"));
}

#[test]
fn unfinished_blocks_are_incomplete_not_wrong() {
    assert!(run_err("repeat 3 times\n  put 1").incomplete);
    assert!(run_err("if 1 = 1 then").incomplete);
    assert!(run_err("on foo\n  put 1").incomplete);
    assert!(!run_err("end repeat").incomplete);
    assert!(!run_err("put 1 +").incomplete);
}

#[test]
fn exit_repeat_outside_a_loop_is_a_syntax_error() {
    assert!(run_err("exit repeat").message.contains("inside a repeat"));
}

// ----- chunk expressions -----------------------------------------------------

#[test]
fn reading_words_items_lines_and_characters() {
    assert_eq!(run("put word 2 of \"alpha beta gamma\""), vec!["beta"]);
    assert_eq!(run("put item 2 of \"a,b,c\""), vec!["b"]);
    assert_eq!(run("put char 3 of \"hello\""), vec!["l"]);
    assert_eq!(run("put character 2 of \"héllo\""), vec!["é"]);
    let script = "put \"one\" & return & \"two\" & return & \"three\" into x\nput line 2 of x";
    assert_eq!(run(script), vec!["two"]);
}

#[test]
fn ordinals() {
    assert_eq!(run("put the first word of \"alpha beta gamma\""), vec!["alpha"]);
    assert_eq!(run("put last word of \"alpha beta gamma\""), vec!["gamma"]);
    assert_eq!(run("put the third item of \"a,b,c,d\""), vec!["c"]);
    assert_eq!(run("put the middle word of \"a b c d e\""), vec!["c"]);
    assert_eq!(run("put the middle word of \"a b c d\""), vec!["c"]);
    assert_eq!(run("put mid item of \"a,b,c\""), vec!["b"]);
}

#[test]
fn ranges() {
    assert_eq!(run("put word 2 to 3 of \"a b c d\""), vec!["b c"]);
    assert_eq!(run("put item 2 to 3 of \"a,b,c,d\""), vec!["b,c"]);
    assert_eq!(run("put char 2 to 4 of \"hello\""), vec!["ell"]);
    assert_eq!(run("put word 3 to 9 of \"a b c d\""), vec!["c d"]);
}

#[test]
fn out_of_range_reads_are_empty() {
    assert_eq!(run("put word 9 of \"a b\""), vec![""]);
    assert_eq!(run("put word 0 of \"a b\""), vec![""]);
    assert_eq!(run("put word 5 to 9 of \"a b c d\""), vec![""]);
    assert_eq!(run("put word 3 to 2 of \"a b c d\""), vec![""]);
}

#[test]
fn nested_chunks() {
    let script = r#"
put "a b c" & return & "d e f g" into x
put word 3 of line 2 of x
put the last word of the first line of x
"#;
    assert_eq!(run(script), vec!["f", "c"]);
}

#[test]
fn of_in_and_from_are_interchangeable() {
    assert_eq!(run("put word 2 in \"a b c\""), vec!["b"]);
    assert_eq!(run("put word 2 from \"a b c\""), vec!["b"]);
}

#[test]
fn an_index_can_be_an_expression() {
    assert_eq!(run("put 1 into i\nput word i + 1 of \"a b c\""), vec!["b"]);
}

#[test]
fn a_chunk_binds_to_the_nearest_operand() {
    assert_eq!(run("put first word of \"a b\" & \"!\""), vec!["a!"]);
}

#[test]
fn counting_chunks() {
    assert_eq!(run("put the number of words in \"a b c\""), vec!["3"]);
    assert_eq!(run("put number of items in \"a,b,,d\""), vec!["4"]);
    assert_eq!(run("put the number of chars in \"héllo\""), vec!["5"]);
    assert_eq!(run("put the number of words in \"\""), vec!["0"]);
    let script = "put \"a\" & return & \"b\" into s\nput the number of lines in s";
    assert_eq!(run(script), vec!["2"]);
}

#[test]
fn chunk_words_are_still_ordinary_variable_names() {
    assert_eq!(run("put 5 into word\nput word"), vec!["5"]);
    assert_eq!(run("put 7 into word\nput word into line\nput line"), vec!["7"]);
    assert_eq!(run("put 3 into first\nput first"), vec!["3"]);
}

#[test]
fn a_line_can_start_with_a_chunk_expression() {
    assert_eq!(run("word 2 of \"a b c\""), vec!["b"]);
    assert_eq!(run("the first word of \"x y\""), vec!["x"]);
}

#[test]
fn a_chunk_can_be_stored() {
    assert_eq!(run("put word 2 of \"a b c\" into y\nput y"), vec!["b"]);
}

#[test]
fn any_picks_one_of_the_chunks() {
    assert_eq!(run("put any word of \"a a a\""), vec!["a"]);
    let out = run("put any item of \"x,y\"");
    assert!(out == vec!["x"] || out == vec!["y"]);
}

#[test]
fn item_delimiter() {
    assert_eq!(run("put the itemDelimiter"), vec![","]);
    assert_eq!(
        run("set the itemDelimiter to \";\"\nput item 2 of \"a;b;c\""),
        vec!["b"]
    );
    assert_eq!(
        run("set the itemDelimiter to \"::\"\nput item 2 of \"a::b::c\""),
        vec!["b"]
    );
    assert!(run_err("set the itemDelimiter to \"\"").message.contains("empty"));
    assert!(run_err("set the colour to 1").message.contains("itemDelimiter"));
}

#[test]
fn putting_into_chunks() {
    let base = "put \"a b c\" into s\n";
    assert_eq!(run(&format!("{base}put \"X\" into word 2 of s\nput s")), vec!["a X c"]);
    assert_eq!(run(&format!("{base}put \"X\" before word 2 of s\nput s")), vec!["a Xb c"]);
    assert_eq!(run(&format!("{base}put \"X\" after word 2 of s\nput s")), vec!["a bX c"]);
    assert_eq!(run(&format!("{base}put \"Z\" into last word of s\nput s")), vec!["a b Z"]);
    assert_eq!(
        run("put \"abc\" into s\nput \"X\" into char 2 of s\nput s"),
        vec!["aXc"]
    );
    assert_eq!(
        run("put \"a b c d\" into s\nput \"X\" into word 2 to 3 of s\nput s"),
        vec!["a X d"]
    );
}

#[test]
fn putting_into_a_chunk_of_a_chunk() {
    let script = r#"
put "one two" & return & "three four" into s
put "X" into word 2 of line 2 of s
put line 1 of s
put line 2 of s
"#;
    assert_eq!(run(script), vec!["one two", "three X"]);
}

#[test]
fn items_and_lines_grow_but_words_do_not() {
    assert_eq!(
        run("put \"a,b\" into s\nput \"z\" into item 4 of s\nput s"),
        vec!["a,b,,z"]
    );
    assert_eq!(run("put \"x\" into item 3 of fresh\nput fresh"), vec![",,x"]);
    assert_eq!(
        run("put \"a\" into s\nput \"z\" into line 3 of s\nput s"),
        vec!["a\n\nz"]
    );
    let err = run_err("put \"a b\" into s\nput \"x\" into word 5 of s");
    assert!(err.message.contains("does not exist"));
}

#[test]
fn you_can_only_put_into_a_variable_or_a_chunk_of_one() {
    assert!(run_err("put 1 into \"abc\"").message.contains("variable"));
}

#[test]
fn looping_over_words() {
    let script = r#"
put "red green blue" into colors
repeat with i = 1 to the number of words in colors
  put word i of colors
end repeat
"#;
    assert_eq!(run(script), vec!["red", "green", "blue"]);
}
