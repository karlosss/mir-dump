// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![feature(box_syntax)]
#![feature(box_patterns)]
#![feature(rustc_private)]

extern crate datafrog;
extern crate rustc;
extern crate rustc_hir;
extern crate rustc_span;
extern crate rustc_hash;
extern crate rustc_index;
extern crate rustc_session;
extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_codegen_utils;

pub mod configuration;
pub mod mir_dumper;
mod mir_analyses;
mod polonius_info;
mod borrowck;
