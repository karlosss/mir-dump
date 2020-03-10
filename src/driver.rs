// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![feature(box_syntax)]
#![feature(rustc_private)]

extern crate rustc_ap_rustc_span as syntax_pos;
extern crate rustc_ap_syntax as syntax;

mod driver_utils;

use crate::driver_utils::run;
use log::{debug, trace, info};
use mir_dump::{configuration, mir_dumper};
use rustc::session;
use rustc_codegen_utils::codegen_backend::CodegenBackend;
use rustc_driver::{driver, getopts, Compilation, CompilerCalls, RustcDefaultCalls};
use syntax::ast;
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

impl<'a> CompilerCalls<'a> for DumperCompilerCalls {
    fn early_callback(
        &mut self,
        matches: &getopts::Matches,
        sopts: &session::config::Options,
        cfg: &ast::CrateConfig,
        descriptions: &rustc_errors::registry::Registry,
        output: session::config::ErrorOutputType,
    ) -> Compilation {
        self.default
            .early_callback(matches, sopts, cfg, descriptions, output)
    }
    fn no_input(
        &mut self,
        matches: &getopts::Matches,
        sopts: &session::config::Options,
        cfg: &ast::CrateConfig,
        odir: &Option<PathBuf>,
        ofile: &Option<PathBuf>,
        descriptions: &rustc_errors::registry::Registry,
    ) -> Option<(session::config::Input, Option<PathBuf>)> {
        self.default
            .no_input(matches, sopts, cfg, odir, ofile, descriptions)
    }
    fn late_callback(
        &mut self,
        trans: &CodegenBackend,
        matches: &getopts::Matches,
        sess: &session::Session,
        crate_stores: &rustc_metadata::cstore::CStore,
        input: &session::config::Input,
        odir: &Option<PathBuf>,
        ofile: &Option<PathBuf>,
    ) -> Compilation {
        if configuration::test() {
            if let rustc::session::config::Input::File(ref path) = input {
                env::set_var("DUMP_TEST_FILE", path.to_str().unwrap());
            }
        }
        self.default
            .late_callback(trans, matches, sess, crate_stores, input, odir, ofile)
    }
    fn build_controller(
        self: Box<Self>,
        sess: &session::Session,
        matches: &getopts::Matches,
    ) -> driver::CompileController<'a> {
        let mut control = self.default.build_controller(sess, matches);

        let old = std::mem::replace(&mut control.after_parse.callback, box |_| {});
        control.after_parse.callback = box move |state| {
            trace!("[after_parse.callback] enter");
            let start = Instant::now();

            // Parse specifications.

            let duration = start.elapsed();
            info!("Parsing of annotations successful ({}.{} seconds)", duration.as_secs(), duration.subsec_millis()/10);
            trace!("[after_parse.callback] exit");
            old(state);
        };

        let old = std::mem::replace(&mut control.after_analysis.callback, box |_| {});
        control.after_analysis.callback = box move |state| {
            trace!("[after_analysis.callback] enter");
            let start = Instant::now();

            // Type-check specifications.

            let duration = start.elapsed();
            info!("Type-checking of annotations successful ({}.{} seconds)", duration.as_secs(), duration.subsec_millis()/10);

            // Call the verifier.
            if configuration::dump_mir_info() {
                mir_dumper::dump_info(state);
            }

            trace!("[after_analysis.callback] exit");
            old(state);
        };

        if !configuration::full_compilation() {
            debug!("The program will not be compiled.");
            control.after_analysis.stop = Compilation::Stop;
        }
        control
    }
}

pub fn main() {
    env_logger::init();

    let exit_status = run(move || {
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

        let compiler_calls = Box::new(DumperCompilerCalls::new());

        debug!("rustc command: '{}'", args.join(" "));
        rustc_driver::run_compiler(&args, compiler_calls, None, None)
    });
    std::process::exit(exit_status as i32);
}
