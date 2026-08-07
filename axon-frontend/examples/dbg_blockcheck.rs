use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let src = std::fs::read_to_string(&path).unwrap();
    let tokens = match Lexer::new(&src, &path).tokenize() {
        Ok(t) => t, Err(e) => { println!("LEX: {e:?}"); return }
    };
    let program = match Parser::new(tokens).parse() {
        Ok(p) => p, Err(e) => { println!("PARSE {}:{}: {}", e.line, e.column, e.message); return }
    };
    let errors = TypeChecker::new(&program).check();
    if errors.is_empty() { println!("CLEAN"); }
    for e in errors { println!("TC: {}", e.message); }
}
