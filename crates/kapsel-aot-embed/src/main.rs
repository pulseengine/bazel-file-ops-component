//! `kapsel-aot-embed embed --input IN --output OUT NAME:PATH...`
//! `kapsel-aot-embed list IN`
//! `kapsel-aot-embed extract IN NAME OUT`
//! `kapsel-aot-embed strip IN OUT`
//!
//! Exit codes: 0 success, 1 failure, 2 usage error.

use std::fs;
use std::process::ExitCode;

const USAGE: &str = "usage: kapsel-aot-embed embed --input IN --output OUT NAME:PATH...
       kapsel-aot-embed list IN
       kapsel-aot-embed extract IN NAME OUT
       kapsel-aot-embed strip IN OUT";

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("{path}: {e}"))
}

fn write(path: &str, data: &[u8]) -> Result<(), String> {
    fs::write(path, data).map_err(|e| format!("{path}: {e}"))
}

fn embed(args: &[String]) -> Result<Option<()>, String> {
    let (mut input, mut output, mut artifacts) = (None, None, Vec::new());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--input" | "-i" => input = it.next().cloned(),
            "--output" | "-o" => output = it.next().cloned(),
            spec => {
                let Some((name, path)) = spec.split_once(':') else {
                    return Err(format!("invalid artifact {spec:?}: expected NAME:PATH"));
                };
                artifacts.push((name.to_string(), read(path)?));
            }
        }
    }
    let (Some(input), Some(output)) = (input, output) else {
        return Ok(None);
    };
    if artifacts.is_empty() {
        return Ok(None);
    }
    let wasm = read(&input)?;
    let out = kapsel_aot_embed::embed(&wasm, &artifacts)?;
    write(&output, &out)?;
    for (name, data) in &artifacts {
        println!("{}{name}: {} bytes", kapsel_aot_embed::PREFIX, data.len());
    }
    println!(
        "{input}: {} bytes -> {output}: {} bytes",
        wasm.len(),
        out.len()
    );
    Ok(Some(()))
}

fn run(args: &[String]) -> Result<Option<()>, String> {
    match args {
        [cmd, rest @ ..] if cmd == "embed" => embed(rest),
        [cmd, input] if cmd == "list" => {
            for (name, r) in kapsel_aot_embed::list(&read(input)?)? {
                println!("{}{name} {}", kapsel_aot_embed::PREFIX, r.len());
            }
            Ok(Some(()))
        }
        [cmd, input, name, output] if cmd == "extract" => {
            let wasm = read(input)?;
            write(output, kapsel_aot_embed::extract(&wasm, name)?)?;
            Ok(Some(()))
        }
        [cmd, input, output] if cmd == "strip" => {
            write(output, &kapsel_aot_embed::strip(&read(input)?)?)?;
            Ok(Some(()))
        }
        _ => Ok(None),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(Some(())) => ExitCode::SUCCESS,
        Ok(None) => usage(),
        Err(e) => {
            eprintln!("kapsel-aot-embed: {e}");
            ExitCode::FAILURE
        }
    }
}
