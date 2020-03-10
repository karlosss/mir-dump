// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![feature(box_syntax)]
#![feature(box_patterns)]
#![feature(rustc_private)]

extern crate datafrog;
extern crate rustc;
extern crate rustc_hash;
extern crate rustc_driver as driver;
extern crate rustc_index;
extern crate rustc_hir;
extern crate rustc_mir;
extern crate rustc_ty;
extern crate rustc_ap_syntax as syntax;
extern crate rustc_ap_rustc_span as syntax_pos;
extern crate rustc_ap_rustc_data_structures as rustc_data_structures;

pub mod configuration;
pub mod mir_dumper;
mod mir_analyses;
mod polonius_info;
mod borrowck;
