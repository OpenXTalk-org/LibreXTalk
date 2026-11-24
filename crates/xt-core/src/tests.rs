//! Tests that run whole scripts through the engine.

use std::time::Duration;

use crate::{Host, Interpreter, XtError};

#[derive(Default)]
struct Capture {
    out: Vec<String>,
    slept: Vec<Duration>,
    /// Prompts shown by `answer` and `ask`, for the tests to check.
    prompts: Vec<String>,
    /// Button choices for `answer`, used in order. Empty means the first button.
    answers: Vec<usize>,
    /// Replies for `ask`, used in order. Empty means accept the default.
    asks: Vec<Option<String>>,
}

impl Host for Capture {
    fn message(&mut self, text: &str) {
        self.out.push(text.to_string());
    }

    fn allow_shell(&mut self, _shell: &str, _script: &str) -> bool {
        true
    }

    fn answer(&mut self, prompt: &str, buttons: &[String]) -> usize {
        self.prompts.push(format!("{prompt}|{}", buttons.join("/")));
        if self.answers.is_empty() {
            0
        } else {
            self.answers.remove(0)
        }
    }

    fn ask(&mut self, prompt: &str, default: &str) -> Option<String> {
        self.prompts.push(format!("{prompt}|{default}"));
        if self.asks.is_empty() {
            Some(default.to_string())
        } else {
            self.asks.remove(0)
        }
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
    assert_eq!(
        run("if 1 < 2 then put \"yes\" else put \"no\""),
        vec!["yes"]
    );
    assert_eq!(run("if 1 > 2 then put \"yes\" else put \"no\""), vec!["no"]);
    assert_eq!(
        run("if 1 > 2 then put \"yes\"\nput \"after\""),
        vec!["after"]
    );
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
    assert_eq!(
        run("repeat 3 times\n  put \"hi\"\nend repeat"),
        vec!["hi", "hi", "hi"]
    );
    assert_eq!(
        run("repeat for 2 times\n  put \"x\"\nend repeat"),
        vec!["x", "x"]
    );
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
on captureArgs a b
  put a && b
end captureArgs

captureArgs "x" "y"
captureArgs 1, 2
captureArgs (1 + 1) "go"
"#;
    assert_eq!(run(script), vec!["x y", "1 2", "2 go"]);
}

#[test]
fn parameters_may_be_separated_by_commas_or_spaces() {
    let script = "on captureArgs a b, c\n  put a & b & c\nend captureArgs\ncaptureArgs 1 2 3";
    assert_eq!(run(script), vec!["123"]);
}

#[test]
fn missing_arguments_are_empty() {
    assert_eq!(
        run("on captureArgs a b\n  put a && b\nend captureArgs\ncaptureArgs \"x\""),
        vec!["x "]
    );
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
    assert!(run_err("put \"abc\" + 1")
        .message
        .contains("Expected a number"));
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
    let script = "on captureArgs a b\n  put a\nend captureArgs\ncaptureArgs 1 + 1 \"go\"";
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
    assert_eq!(
        run("put the first word of \"alpha beta gamma\""),
        vec!["alpha"]
    );
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
    assert_eq!(
        run("put 7 into word\nput word into line\nput line"),
        vec!["7"]
    );
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
    assert!(run_err("set the itemDelimiter to \"\"")
        .message
        .contains("empty"));
    assert!(run_err("set the colour to 1")
        .message
        .contains("itemDelimiter"));
}

#[test]
fn putting_into_chunks() {
    let base = "put \"a b c\" into s\n";
    assert_eq!(
        run(&format!("{base}put \"X\" into word 2 of s\nput s")),
        vec!["a X c"]
    );
    assert_eq!(
        run(&format!("{base}put \"X\" before word 2 of s\nput s")),
        vec!["a Xb c"]
    );
    assert_eq!(
        run(&format!("{base}put \"X\" after word 2 of s\nput s")),
        vec!["a bX c"]
    );
    assert_eq!(
        run(&format!("{base}put \"Z\" into last word of s\nput s")),
        vec!["a b Z"]
    );
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
    assert_eq!(
        run("put \"x\" into item 3 of fresh\nput fresh"),
        vec![",,x"]
    );
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

// ----- built-in functions ----------------------------------------------------

#[test]
fn numeric_functions() {
    assert_eq!(run("put abs(-3)"), vec!["3"]);
    assert_eq!(run("put round(2.5)"), vec!["3"]);
    assert_eq!(run("put round(-2.5)"), vec!["-3"]);
    assert_eq!(run("put trunc(2.9)"), vec!["2"]);
    assert_eq!(run("put sqrt(16)"), vec!["4"]);
    assert_eq!(run("put min(3, 1, 2)"), vec!["1"]);
    assert_eq!(run("put max(3, 1, 2)"), vec!["3"]);
    assert_eq!(run("put mod(7, 2)"), vec!["1"]);
    assert_eq!(run("put div(7, 2)"), vec!["3"]);
    assert_eq!(run("put random(1)"), vec!["1"]);
}

#[test]
fn random_stays_in_range() {
    for _ in 0..20 {
        let n: i64 = run("put random(6)")[0].parse().unwrap();
        assert!((1..=6).contains(&n));
    }
}

#[test]
fn text_functions() {
    assert_eq!(run("put length(\"héllo\")"), vec!["5"]);
    assert_eq!(run("put the length of \"abc\""), vec!["3"]);
    assert_eq!(run("put offset(\"lo\", \"hello\")"), vec!["4"]);
    assert_eq!(run("put offset(\"z\", \"hello\")"), vec!["0"]);
    assert_eq!(run("put toUpper(\"abc\")"), vec!["ABC"]);
    assert_eq!(run("put toLower(\"ABC\")"), vec!["abc"]);
    assert_eq!(run("put numToChar(65)"), vec!["A"]);
    assert_eq!(run("put charToNum(\"A\")"), vec!["65"]);
}

#[test]
fn time_functions() {
    assert_eq!(run("put the seconds > 1700000000"), vec!["true"]);
    assert_eq!(run("put seconds() > 1700000000"), vec!["true"]);
    assert_eq!(run("put the milliseconds > the seconds"), vec!["true"]);
}

#[test]
fn function_errors() {
    assert!(run_err("put abs()").message.contains("expects"));
    assert!(run_err("put abs(1, 2)").message.contains("expects"));
    assert!(run_err("put sqrt(-1)").message.contains("square root"));
    assert!(run_err("put abs(\"x\")")
        .message
        .contains("Expected a number"));
}

#[test]
fn a_user_function_beats_a_built_in() {
    let script = "function length x\n  return \"mine\"\nend length\nput length(\"abc\")";
    assert_eq!(run(script), vec!["mine"]);
}

#[test]
fn parameter_functions() {
    let script = r#"
on captureArgs a b
  put paramCount() & ":" & param(1) & ":" & param(0) & ":" & params()
end captureArgs
captureArgs "x" "y" "z"
"#;
    assert_eq!(run(script), vec!["3:x:captureArgs:x,y,z"]);
}

// ----- arithmetic commands ---------------------------------------------------

#[test]
fn arithmetic_commands() {
    let script = r#"
put 5 into n
add 3 to n
subtract 1 from n
multiply n by 2
divide n by 4
put n
"#;
    assert_eq!(run(script), vec!["3.5"]);
}

#[test]
fn arithmetic_on_a_chunk() {
    let script = "put \"1,2,3\" into s\nadd 1 to item 2 of s\nput s";
    assert_eq!(run(script), vec!["1,3,3"]);
}

#[test]
fn arithmetic_on_an_unassigned_variable_is_an_error() {
    assert!(run_err("add 1 to total")
        .message
        .contains("Expected a number"));
}

// ----- delete ----------------------------------------------------------------

#[test]
fn deleting_chunks() {
    assert_eq!(
        run("put \"a b c\" into s\ndelete word 2 of s\nput s"),
        vec!["a c"]
    );
    assert_eq!(
        run("put \"a,b,c\" into s\ndelete last item of s\nput s"),
        vec!["a,b"]
    );
    assert_eq!(
        run("put \"a,b,c\" into s\ndelete item 1 of s\nput s"),
        vec!["b,c"]
    );
    assert_eq!(
        run("put \"x\" & return & \"y\" into s\ndelete line 1 of s\nput s"),
        vec!["y"]
    );
    assert_eq!(
        run("put \"hello\" into s\ndelete char 2 to 3 of s\nput s"),
        vec!["hlo"]
    );
    assert_eq!(
        run("put \"a b c\" into s\ndelete word 9 of s\nput s"),
        vec!["a b c"]
    );
    assert_eq!(
        run("put \"solo\" into s\ndelete word 1 of s\nput s"),
        vec![""]
    );
}

#[test]
fn deleting_a_chunk_of_a_chunk() {
    let script =
        "put \"a b\" & return & \"c d\" into s\ndelete word 1 of line 2 of s\nput line 2 of s";
    assert_eq!(run(script), vec!["d"]);
}

#[test]
fn delete_needs_a_chunk() {
    assert!(run_err("put 1 into x\ndelete x").message.contains("chunk"));
}

// ----- try, catch, throw -----------------------------------------------------

#[test]
fn catching_an_error() {
    let script = r#"
try
  put 1 / 0
catch e
  put "caught:" && e
end try
put "after"
"#;
    assert_eq!(run(script), vec!["caught: Can't divide by zero", "after"]);
}

#[test]
fn a_try_that_does_not_fail_skips_the_catch() {
    let script = "try\n  put \"ok\"\ncatch e\n  put \"bad\"\nend try";
    assert_eq!(run(script), vec!["ok"]);
}

#[test]
fn throw_and_catch() {
    let script = "try\n  throw \"boom\"\ncatch err\n  put err\nend try";
    assert_eq!(run(script), vec!["boom"]);
    assert_eq!(run_err("throw \"bad\"").message, "bad");
}

#[test]
fn an_error_in_a_called_handler_is_caught_by_the_caller() {
    let script = r#"
on inner
  put 1 / 0
end inner

on outer
  inner
  put "not reached"
end outer

try
  outer
catch e
  put "caught"
end try
"#;
    assert_eq!(run(script), vec!["caught"]);
}

#[test]
fn runaway_recursion_can_be_caught() {
    let script = "on loopy\n  loopy\nend loopy\ntry\n  loopy\ncatch e\n  put \"caught\"\nend try";
    assert_eq!(run(script), vec!["caught"]);
}

#[test]
fn leaving_a_loop_closes_its_try() {
    // If `exit repeat` left the try open, the division error below would be
    // caught by the stale catch block instead of escaping.
    let script = r#"
repeat 2 times
  try
    exit repeat
  catch e
    put "x"
  end try
end repeat
put 1 / 0
"#;
    assert_eq!(run_err(script).message, "Can't divide by zero");
}

#[test]
fn try_needs_a_catch() {
    assert!(run_err("try\n  put 1\nend try").message.contains("catch"));
    assert!(run_err("try\n  put 1").incomplete);
    assert!(run_err("catch e").message.contains("Unexpected"));
}

// ----- answer and ask --------------------------------------------------------

#[test]
fn answer_stores_the_chosen_button() {
    let mut interp = Interpreter::new();
    let mut host = Capture {
        answers: vec![1],
        ..Capture::default()
    };
    let script = "answer \"Continue?\" with \"Yes\" or \"No\" or \"Cancel\"\nput it";
    interp.run_source(script, &mut host).unwrap();
    assert_eq!(host.out, vec!["No"]);
    assert_eq!(host.prompts, vec!["Continue?|Yes/No/Cancel"]);
}

#[test]
fn answer_without_buttons_has_ok() {
    let mut interp = Interpreter::new();
    let mut host = Capture::default();
    interp
        .run_source("answer \"Hi\"\nput it", &mut host)
        .unwrap();
    assert_eq!(host.out, vec!["OK"]);
    assert_eq!(host.prompts, vec!["Hi|OK"]);
}

#[test]
fn ask_stores_the_reply() {
    let mut interp = Interpreter::new();
    let mut host = Capture {
        asks: vec![Some("Bob".to_string())],
        ..Capture::default()
    };
    interp
        .run_source("ask \"Name?\" with \"Anon\"\nput it", &mut host)
        .unwrap();
    assert_eq!(host.out, vec!["Bob"]);
    assert_eq!(host.prompts, vec!["Name?|Anon"]);
}

#[test]
fn cancelling_ask_sets_the_result() {
    let mut interp = Interpreter::new();
    let mut host = Capture {
        asks: vec![None],
        ..Capture::default()
    };
    interp
        .run_source("ask \"Name?\"\nput it & \"/\" & the result", &mut host)
        .unwrap();
    assert_eq!(host.out, vec!["/Cancel"]);
}

// ----- semicolons ------------------------------------------------------------

#[test]
fn a_semicolon_separates_statements() {
    assert_eq!(run("put \"a\"; put \"b\""), vec!["a", "b"]);
    assert_eq!(run("put 1 into x; put x + 1"), vec!["2"]);
    assert_eq!(run("put \"a\";; put \"b\";"), vec!["a", "b"]);
}

#[test]
fn a_semicolon_inside_a_string_or_comment_is_not_a_separator() {
    assert_eq!(run("put \"a;b\""), vec!["a;b"]);
    assert_eq!(run("put \"a\" -- note; put \"b\""), vec!["a"]);
    assert_eq!(
        run("set the itemDelimiter to \";\"; put item 2 of \"a;b;c\""),
        vec!["b"]
    );
}

#[test]
fn blocks_and_handlers_fit_on_one_line() {
    assert_eq!(
        run("repeat 3 times; put \"hi\"; end repeat"),
        vec!["hi", "hi", "hi"]
    );
    assert_eq!(run("on greet; put \"hi\"; end greet; greet"), vec!["hi"]);
    assert_eq!(
        run("if 1 = 1 then; put \"yes\"; else; put \"no\"; end if"),
        vec!["yes"]
    );
    assert_eq!(
        run("repeat with i = 1 to 3; put i; end repeat"),
        vec!["1", "2", "3"]
    );
}

#[test]
fn errors_after_a_semicolon_point_at_the_right_place() {
    let err = run_err("put 1; put");
    assert_eq!((err.line, err.col), (Some(0), Some(10)));
    let err = run_err("put 1; put 1 / 0");
    assert_eq!(err.line, Some(0));
}

// ----- the cr constant -------------------------------------------------------

#[test]
fn cr_is_a_newline() {
    assert_eq!(run("put \"a\" & CR & \"b\""), vec!["a\nb"]);
    assert_eq!(run("put \"a\" & cr & \"b\""), vec!["a\nb"]);
    assert_eq!(run("put \"a\" & Cr & \"b\""), vec!["a\nb"]);
    assert_eq!(run("put cr = return"), vec!["true"]);
    assert_eq!(run("put cr = linefeed"), vec!["true"]);
    assert_eq!(run("put length(cr)"), vec!["1"]);
    assert_eq!(
        run("put the number of lines in (\"a\" & cr & \"b\")"),
        vec!["2"]
    );
    assert_eq!(run("put line 2 of (\"a\" & CR & \"b\")"), vec!["b"]);
}

#[test]
fn constants_cannot_be_assigned() {
    assert!(run_err("put 5 into cr").message.contains("constant"));
    assert!(run_err("put 5 into return").message.contains("constant"));
    assert!(run_err("add 1 to pi").message.contains("constant"));
    assert!(run_err("put 1 into word 2 of space")
        .message
        .contains("constant"));
    assert!(run_err("repeat with space = 1 to 3; put 1; end repeat")
        .message
        .contains("constant"));
}

// ----- more character constants ----------------------------------------------

#[test]
fn character_constants() {
    assert_eq!(run("put \"a\" & tab & \"b\""), vec!["a\tb"]);
    assert_eq!(run("put \"a\" & comma & \"b\""), vec!["a,b"]);
    assert_eq!(run("put \"a\" & colon & \"b\""), vec!["a:b"]);
    assert_eq!(run("put \"a\" & slash & \"b\""), vec!["a/b"]);
    assert_eq!(run("put \"a\" & backslash & \"b\""), vec!["a\\b"]);
    assert_eq!(run("put \"a\" & space & \"b\""), vec!["a b"]);
    assert_eq!(run("put quote & \"hi\" & quote"), vec!["\"hi\""]);
    assert_eq!(run("put \"a\" & crlf & \"b\""), vec!["a\r\nb"]);
}

#[test]
fn character_constants_have_the_right_codes() {
    assert_eq!(run("put charToNum(tab)"), vec!["9"]);
    assert_eq!(run("put charToNum(return)"), vec!["10"]);
    assert_eq!(run("put charToNum(cr)"), vec!["10"]);
    assert_eq!(run("put charToNum(lf)"), vec!["10"]);
    assert_eq!(run("put charToNum(linefeed)"), vec!["10"]);
    assert_eq!(run("put charToNum(formfeed)"), vec!["12"]);
    assert_eq!(run("put charToNum(eof)"), vec!["4"]);
    assert_eq!(run("put charToNum(null)"), vec!["0"]);
    assert_eq!(run("put charToNum(space)"), vec!["32"]);
    assert_eq!(run("put charToNum(quote)"), vec!["34"]);
    assert_eq!(run("put charToNum(comma)"), vec!["44"]);
    assert_eq!(run("put charToNum(colon)"), vec!["58"]);
    assert_eq!(run("put charToNum(slash)"), vec!["47"]);
    assert_eq!(run("put charToNum(backslash)"), vec!["92"]);
    assert_eq!(run("put length(crlf)"), vec!["2"]);
    assert_eq!(run("put lf = linefeed"), vec!["true"]);
}

#[test]
fn constants_are_case_insensitive() {
    assert_eq!(run("put length(CRLF)"), vec!["2"]);
    assert_eq!(run("put TAB = tab"), vec!["true"]);
    assert_eq!(run("put Comma = comma"), vec!["true"]);
}

#[test]
fn a_tab_can_be_the_item_delimiter() {
    let script = "set the itemDelimiter to tab\nput item 2 of (\"a\" & tab & \"b\" & tab & \"c\")";
    assert_eq!(run(script), vec!["b"]);
}

#[test]
fn the_new_constants_cannot_be_assigned_either() {
    assert!(run_err("put 1 into null").message.contains("constant"));
    assert!(run_err("put 1 into slash").message.contains("constant"));
    assert!(run_err("put 1 into crlf").message.contains("constant"));
}

// ----- shell access ----------------------------------------------------------

/// A host that implements only what is required, so it denies shell access.
struct Silent;

impl Host for Silent {
    fn message(&mut self, _text: &str) {}
}

#[test]
fn shell_access_is_denied_unless_the_host_allows_it() {
    let mut interp = Interpreter::new();
    let mut host = Silent;
    let err = interp
        .run_source("put shell(\"echo hi\")", &mut host)
        .unwrap_err();
    assert!(err.message.contains("does not allow"));
    let err = interp
        .run_source("do \"echo hi\" as sh", &mut host)
        .unwrap_err();
    assert!(err.message.contains("does not allow"));
}

#[test]
fn do_needs_a_known_shell() {
    assert!(run_err("do \"ls\" as klingon")
        .message
        .contains("Unknown language"));
    assert!(run_err("do \"ls\"").message.contains("as"));
}

#[cfg(unix)]
mod shell_tests {
    use super::*;
    use crate::Status;

    #[test]
    fn the_shell_function_returns_standard_output() {
        assert_eq!(run("put shell(\"printf hello\")"), vec!["hello"]);
        assert_eq!(run("put shell(\"echo hi\")"), vec!["hi\n"]);
        assert_eq!(run("put word 2 of shell(\"printf 'a b c'\")"), vec!["b"]);
        assert_eq!(run("put the length of shell(\"uname\") > 0"), vec!["true"]);
    }

    #[test]
    fn the_shell_function_sets_the_result_to_the_exit_status() {
        assert_eq!(
            run("put shell(\"exit 3\") & \"/\" & the result"),
            vec!["/3"]
        );
        assert_eq!(run("put shell(\"true\") & the result"), vec!["0"]);
    }

    #[test]
    fn standard_error_is_discarded() {
        assert_eq!(run("put shell(\"printf out; echo err 1>&2\")"), vec!["out"]);
        assert_eq!(
            run("put shell(\"printf out; echo err 1>&2\") is \"out\""),
            vec!["true"]
        );
    }

    #[test]
    fn do_as_shell_stores_output_in_it() {
        assert_eq!(run("do \"printf hi\" as sh\nput it"), vec!["hi"]);
        assert_eq!(run("do \"printf hi\" as shell\nput it"), vec!["hi"]);
        assert_eq!(run("do \"exit 2\" as sh\nput the result"), vec!["2"]);
    }

    #[test]
    fn do_as_shell_pauses_the_script_but_not_the_host() {
        let mut interp = Interpreter::new();
        let mut host = Capture::default();
        interp
            .load("put \"before\"\ndo \"sleep 0.2; printf done\" as sh\nput it")
            .unwrap();
        let status = interp.run(&mut host).unwrap();
        assert!(matches!(status, Status::Sleeping(_)));
        assert_eq!(host.out, vec!["before"]);
        // Keep resuming, as a host's event loop would.
        loop {
            match interp.run(&mut host).unwrap() {
                Status::Finished => break,
                Status::Sleeping(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        assert_eq!(host.out, vec!["before", "done"]);
    }

    #[test]
    fn a_missing_command_is_an_exit_status_not_an_error() {
        let script = "put shell(\"nosuchcommand_xyz 2>/dev/null\") & \"/\" & the result";
        assert_eq!(run(script), vec!["/127"]);
    }
}

// ----- the object model ------------------------------------------------------

#[test]
fn a_new_engine_has_one_stack_with_one_card() {
    assert_eq!(run("put the number of stacks"), vec!["1"]);
    assert_eq!(run("put the number of cards"), vec!["1"]);
    assert_eq!(run("put the number of backgrounds"), vec!["1"]);
    assert_eq!(run("put the name of this stack"), vec!["Untitled"]);
    assert_eq!(run("put the number of this card"), vec!["1"]);
}

#[test]
fn the_object_tree_can_be_listed() {
    let interp = Interpreter::new();
    let listing = interp.describe_objects();
    assert!(listing.contains("stack id 1 \"Untitled\" *"));
    assert!(listing.contains("card id 3 \"\" *"));
}

#[test]
fn creating_and_reading_controls() {
    assert_eq!(run("create button \"OK\"\nput it"), vec!["button id 4"]);
    let script = "create button \"OK\"\nput the name of button 1\nput the number of buttons";
    assert_eq!(run(script), vec!["OK", "1"]);
    assert_eq!(
        run("create button \"OK\"\nput the name of button id 4"),
        vec!["OK"]
    );
}

#[test]
fn objects_by_number_name_and_position() {
    let script = r#"
create button "A"
create button "B"
put the name of button 2
put the name of button "A"
put the name of the first button
put the name of last button
"#;
    assert_eq!(run(script), vec!["B", "A", "A", "B"]);
}

#[test]
fn a_field_is_a_container() {
    let script = r#"
create field "msg"
put "hello world" into field "msg"
put field "msg"
put word 2 of field "msg"
put the text of field 1
put "big" before field "msg"
put field "msg"
put "X" into word 2 of field "msg"
put field 1
"#;
    assert_eq!(
        run(script),
        vec![
            "hello world",
            "world",
            "hello world",
            "bighello world",
            "bighello X"
        ]
    );
}

#[test]
fn arithmetic_and_delete_work_on_fields() {
    let script =
        "create field \"n\"\nput 5 into field \"n\"\nadd 2 to field \"n\"\nput field \"n\"";
    assert_eq!(run(script), vec!["7"]);
    let script = "create field \"f\"\nput \"a b c\" into field \"f\"\ndelete word 2 of field \"f\"\nput field \"f\"";
    assert_eq!(run(script), vec!["a c"]);
}

#[test]
fn only_fields_hold_text() {
    let err = run_err("create button \"B\"\nput \"x\" into button 1");
    assert!(err.message.contains("only put text into a field"));
}

#[test]
fn rectangles() {
    let script = r#"
create button "B"
set the rect of button 1 to "10,20,110,50"
put the width of button 1
put the height of button 1
put the left of button 1
set the left of button 1 to 50
put the rect of button 1
set the width of button 1 to 40
put the rect of button 1
set the loc of button 1 to "100,100"
put the rect of button 1
"#;
    assert_eq!(
        run(script),
        vec![
            "100",
            "30",
            "10",
            "50,20,150,50",
            "50,20,90,50",
            "80,85,120,115"
        ]
    );
}

#[test]
fn show_hide_and_boolean_properties() {
    let script = "create button \"B\"\nput the visible of button 1\nhide button 1\nput the visible of button 1\nshow button 1\nput the visible of button 1";
    assert_eq!(run(script), vec!["true", "false", "true"]);
    let err = run_err("create button \"B\"\nset the visible of button 1 to maybe");
    assert!(err.message.contains("true or false"));
}

#[test]
fn custom_properties() {
    let script = "create button \"B\"\nput the colour of button 1 & \"|\"\nset the colour of button 1 to red\nput the colour of button 1";
    assert_eq!(run(script), vec!["|", "red"]);
}

#[test]
fn some_properties_are_read_only() {
    assert!(run_err("set the id of this card to 5")
        .message
        .contains("can't be changed"));
    assert_eq!(
        run("create button \"B\"\nput the id of button 1 > 0"),
        vec!["true"]
    );
    assert_eq!(
        run("create button \"B\"\nput the number of button 1"),
        vec!["1"]
    );
}

#[test]
fn objects_inside_other_objects() {
    let script = r#"
create card
create button "Two"
go to card 1
put the name of button 1 of card 2
put the number of buttons of card 2
put the number of buttons
"#;
    assert_eq!(run(script), vec!["Two", "1", "0"]);
}

#[test]
fn an_object_reference_can_be_kept_in_a_variable() {
    let script =
        "create button \"A\"\nput it into r\nset the name of r to \"Z\"\nput the name of button 1";
    assert_eq!(run(script), vec!["Z"]);
}

#[test]
fn moving_between_cards() {
    let script = r#"
create card
put the number of cards
put the number of this card
go to card 1
put the number of this card
go next
put the number of this card
go previous
put the number of this card
go last
put the number of this card
go first
put the number of this card
"#;
    assert_eq!(run(script), vec!["2", "2", "1", "2", "1", "2", "1"]);
}

#[test]
fn going_to_a_card_sends_close_and_open_messages() {
    let script = r#"
create card
set the script of card 1 to "on closeCard; put leaving; end closeCard"
set the script of card 2 to "on openCard; put arriving; end openCard"
go to card 1
go to card 2
"#;
    assert_eq!(run(script), vec!["leaving", "arriving"]);
}

#[test]
fn several_stacks() {
    let script = r#"
create stack "Second"
put the name of this stack
put the number of stacks
go to stack "Untitled"
put the name of this stack
put the number of this card
"#;
    assert_eq!(run(script), vec!["Second", "2", "Untitled", "1"]);
}

#[test]
fn deleting_objects() {
    let script = "create button \"A\"\ncreate button \"B\"\ndelete button 1\nput the number of buttons\nput the name of button 1";
    assert_eq!(run(script), vec!["1", "B"]);
    assert!(run_err("delete card 1").message.contains("last card"));
    assert_eq!(
        run("create card\ndelete card 2\nput the number of this card"),
        vec!["1"]
    );
    let err = run_err("create button \"A\"\nput it into r\ndelete button 1\nput the name of r");
    assert!(err.message.contains("no longer exists"));
}

#[test]
fn object_errors() {
    assert!(run_err("put the name of button 9")
        .message
        .contains("There is no button"));
    assert!(run_err("put the name of x")
        .message
        .contains("Expected an object"));
    assert!(run_err("create button \"A\" in card 9")
        .message
        .contains("There is no card"));
}

// ----- object scripts and messages -------------------------------------------

#[test]
fn a_button_script_handles_a_message() {
    let script = r#"
create button "OK"
set the script of button 1 to "on mouseUp; put clicked; end mouseUp"
send mouseUp to button 1
"#;
    assert_eq!(run(script), vec!["clicked"]);
    assert_eq!(
        run("create button \"OK\"\nset the script of button 1 to \"on mouseUp; put clicked; end mouseUp\"\nput the script of button 1"),
        vec!["on mouseUp; put clicked; end mouseUp"]
    );
}

#[test]
fn a_message_travels_up_the_path() {
    let script = r#"
create button "OK"
set the script of this card to "on mouseUp; put handledbycard; end mouseUp"
send mouseUp to button 1
"#;
    assert_eq!(run(script), vec!["handledbycard"]);
}

#[test]
fn pass_sends_the_message_on() {
    let script = r#"
create button "OK"
set the script of this card to "on mouseUp; put second; end mouseUp"
set the script of button 1 to "on mouseUp; put first; pass mouseUp; end mouseUp"
send mouseUp to button 1
"#;
    assert_eq!(run(script), vec!["first", "second"]);
    let script = r#"
create button "OK"
set the script of button 1 to "on mouseUp; put first; pass mouseUp; put never; end mouseUp"
send mouseUp to button 1
"#;
    assert_eq!(run(script), vec!["first"]);
}

#[test]
fn me_and_the_target() {
    let script = r#"
create button "OK"
set the script of this card to "on mouseUp; put the name of the target; end mouseUp"
send mouseUp to button "OK"
"#;
    assert_eq!(run(script), vec!["OK"]);
    let script = r#"
create button "OK"
set the script of button 1 to "on mouseUp; put the name of me; end mouseUp"
send mouseUp to button 1
"#;
    assert_eq!(run(script), vec!["OK"]);
    assert!(run_err("put the name of me").message.contains("'me'"));
}

#[test]
fn engine_level_handlers_are_the_last_stop() {
    let script = "on hello\n  put hi\nend hello\ncreate button \"OK\"\nsend hello to button 1";
    assert_eq!(run(script), vec!["hi"]);
}

#[test]
fn unhandled_messages() {
    let script = "create button \"OK\"\nsend mouseUp to button 1\nput done";
    assert_eq!(run(script), vec!["done"]);
    let err = run_err("create button \"OK\"\nsend nosuchmessage to button 1");
    assert!(err.message.contains("Can't understand"));
}

#[test]
fn a_script_can_call_a_handler_further_up() {
    let script = r#"
create button "OK"
set the script of this card to "on helper; put helped; end helper"
set the script of button 1 to "on mouseUp; helper; end mouseUp"
send mouseUp to button 1
helper
"#;
    assert_eq!(run(script), vec!["helped", "helped"]);
}

#[test]
fn an_object_script_can_define_a_function() {
    let script = "set the script of this card to \"function double n; return n * 2; end double\"\nput double(4)";
    assert_eq!(run(script), vec!["8"]);
}

#[test]
fn script_errors_are_reported_when_the_script_is_set() {
    let err = run_err("set the script of this card to \"on foo; put 1\"");
    assert!(err.message.contains("script has an error"));
    let err = run_err("set the script of this card to \"put 1\"");
    assert!(err.message.contains("only contain handlers"));
}
