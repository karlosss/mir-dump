// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![feature(box_syntax)]
#![feature(rustc_private)]

extern crate rustc_span as syntax_pos;
extern crate rustc_codegen_utils;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_metadata;
extern crate rustc_interface;
extern crate cute_log;
pub extern crate rustc_session;

// mod driver_utils;

use log::{debug, trace, info};
use mir_dump::{configuration, mir_dumper};
use rustc_codegen_utils::codegen_backend::CodegenBackend;
use rustc_driver::{getopts, Compilation, RustcDefaultCalls, Callbacks, catch_fatal_errors};
use rustc_session::{config, Session, parse};
use rustc_metadata::creader;
use rustc_interface::interface::Compiler;
use rustc_interface::Queries;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

pub fn current_sysroot() -> Option<String> {
    option_env!("SYSROOT")
        .map(String::from)
        .or_else(|| env::var("SYSROOT").ok())
        .or_else(|| {
            let home = option_env!("RUSTUP_HOME").or(option_env!("MULTIRUST_HOME"));
            let toolchain = option_env!("RUSTUP_TOOLCHAIN").or(option_env!("MULTIRUST_TOOLCHAIN"));
            home.and_then(|home| toolchain.map(|toolchain| format!("{}/toolchains/{}", home, toolchain)))
        })
        .or_else(|| {
            Command::new("rustc")
                .arg("--print")
                .arg("sysroot")
                .output()
                .ok()
                .and_then(|out| String::from_utf8(out.stdout).ok())
                .map(|s| s.trim().to_owned())
        })
}

struct DumperCompilerCalls {
    default: Box<RustcDefaultCalls>,
}

impl DumperCompilerCalls {
    fn new() -> Self {
        Self {
            default: Box::new(RustcDefaultCalls),
        }
    }
}

impl Callbacks for DumperCompilerCalls {
    fn after_parsing<'tcx>(&mut self, _compiler: &Compiler, _queries: &'tcx Queries<'tcx>)
        -> Compilation{
        trace!("[after_parsing.callback] enter");

        // Parse specifications: the original mir-dump does nothing here.

        trace!("[after_parsing.callback] exit");
        Compilation::Continue
    }

    fn after_analysis<'tcx>(&mut self, _compiler: &Compiler, _queries: &'tcx Queries<'tcx>)
        -> Compilation{
        trace!("[after_analysis.callback] enter");

        // Type-check specifications: the original mir-dump does nothing here.

        if configuration::dump_mir_info() {
            _queries.global_ctxt()
                .unwrap()
                .peek_mut()
                .enter(|tcx| mir_dumper::dump_info(tcx));
        }

        trace!("[after_analysis.callback] exit");

        if !configuration::full_compilation() {
            debug!("The program will not be compiled.");
            Compilation::Stop
        }
        else{
            Compilation::Continue
        }
    }
}

pub fn main() {
    cute_log::init().expect("failed to initialize log");

    let exit_status = catch_fatal_errors(move || {
        let mut args: Vec<String> = env::args().collect();

        if args.len() <= 1 {
            std::process::exit(1);
        }

        // Setting RUSTC_WRAPPER causes Cargo to pass 'rustc' as the first argument.
        // We're invoking the compiler programmatically, so we ignore this
        if Path::new(&args[1]).file_stem() == Some("rustc".as_ref()) {
            args.remove(1);
        }

        // this conditional check for the --sysroot flag is there so users can call
        // `mir-dumper` directly without having to pass --sysroot or anything
        if !args.iter().any(|s| s == "--sysroot") {
            let sys_root = current_sysroot()
                .expect("need to specify SYSROOT env var during compilation, or use rustup or multirust");
            debug!("Using sys_root='{}'", sys_root);
            args.push("--sysroot".to_owned());
            args.push(sys_root);
        };

        // Arguments required by dumper (Rustc may produce different MIR)
        env::set_var("POLONIUS_ALGORITHM", "Naive");
        args.push("-Zborrowck=mir".to_owned());
        args.push("-Zpolonius".to_owned());
        args.push("-Znll-facts".to_owned());
        args.push("-Zidentify-regions".to_owned());
        args.push("-Zdump-mir-dir=log/mir/".to_owned());
        args.push("-Zdump-mir=renumber".to_owned());
        if configuration::dump_debug_info() {
            args.push("-Zdump-mir=all".to_owned());
            args.push("-Zdump-mir-graphviz".to_owned());
        }
        args.push("-A".to_owned());
        args.push("unused_comparisons".to_owned());

        args.push("--cfg".to_string());
        args.push(r#"feature="mir_dumper""#.to_string());

        let mut compiler_calls = DumperCompilerCalls::new();

        debug!("rustc command: '{}'", args.join(" "));
        rustc_driver::run_compiler(&args, &mut compiler_calls, None, None)
    }).and_then(|exit_status| exit_status);

    let exit_status = match exit_status {
        Ok(_) => rustc_driver::EXIT_SUCCESS,
        Err(_) => rustc_driver::EXIT_FAILURE,
    };
    std::process::exit(exit_status);
}
