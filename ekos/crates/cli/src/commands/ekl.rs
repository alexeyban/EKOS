use super::store::open_store;
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_ekl::{EklInterpreter, ekl_parse, interpreter::default_returns};
use ekos_runtime::Runtime;
use std::path::Path;

pub fn run(config: &EkosConfig, cwd: &Path, query: &str, json: bool) -> Result<()> {
    let ast = match ekl_parse(query) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("{query}");
            eprintln!("{}^", " ".repeat(e.position));
            eprintln!("error: {}", e.message);
            std::process::exit(1);
        }
    };

    let ledger = open_store(config, cwd)?;
    let runtime = Runtime::over(&*ledger);
    let interpreter = EklInterpreter::new(&runtime);

    let result = match interpreter.execute(&ast) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&result.rows)?);
        return Ok(());
    }

    let columns = if ast.returns.is_empty() {
        default_returns(&ast.entity)
    } else {
        ast.returns.clone()
    };

    if result.rows.is_empty() {
        println!("0 rows.");
        return Ok(());
    }

    println!("{}", columns.join("\t"));
    for row in &result.rows {
        let cells: Vec<String> = columns
            .iter()
            .map(|c| row.get(c).map(render_cell).unwrap_or_default())
            .collect();
        println!("{}", cells.join("\t"));
    }
    println!("\n{} row(s).", result.rows.len());

    Ok(())
}

fn render_cell(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a.iter().map(render_cell).collect::<Vec<_>>().join(","),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}
