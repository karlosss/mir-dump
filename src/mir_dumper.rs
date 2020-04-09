// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use log::trace;
use rustc_hir::{self, intravisit};
use rustc::hir;
use rustc::mir;
use rustc::ty::{self, TyCtxt};
use rustc_span;
use rustc_interface::Queries;
use rustc_session::config::BorrowckMode;
use std::cell;
use std::fs::File;
use std::io::{self, Write, BufWriter};
use std::path::PathBuf;
use super::borrowck::facts;
use super::mir_analyses::initialization::{
    compute_definitely_initialized,
    DefinitelyInitializedAnalysisResult
};
use crate::polonius_info::PoloniusInfo;
use crate::configuration;

pub fn dump_info(tcx: TyCtxt) {
    trace!("[dump_info] enter");

    assert!(matches!(tcx.borrowck_mode(), BorrowckMode::Mir), "NLL is not enabled.");
    let mut printer = InfoPrinter {
        tcx,
    };
    intravisit::walk_crate(&mut printer, tcx.hir().krate());

    trace!("[dump_info] exit");
}

struct InfoPrinter<'tcx> {
    pub tcx: TyCtxt<'tcx>,
}

impl<'tcx> intravisit::Visitor<'tcx> for InfoPrinter<'tcx> {
    type Map = hir::map::Map<'tcx>;

    fn nested_visit_map(&mut self) -> intravisit::NestedVisitorMap<hir::map::Map<'tcx>> {
        let map = &self.tcx.hir();
        intravisit::NestedVisitorMap::All(map)
    }

    fn visit_fn(&mut self, fk: intravisit::FnKind<'tcx>, _fd: &'tcx rustc_hir::FnDecl,
                _b: rustc_hir::BodyId, _s: rustc_span::Span, hir_id: rustc_hir::HirId) {
        let name = match fk {
            intravisit::FnKind::ItemFn(name, ..) => name,
            _ => return,
        };
        if name.to_string().ends_with("__spec") {
            // We ignore spec functions.
            return;
        }

        trace!("[visit_fn] enter name={:?}", name);

        match configuration::dump_mir_proc() {
            Some(value) => {
                if name != rustc_span::symbol::Ident::from_str(&value) {
                    return;
                }
            },
            _ => {},
        };

        let def_id = self.tcx.hir().local_def_id(hir_id);
        self.tcx.mir_borrowck(def_id);

        // Read Polonius facts.
        let def_path = self.tcx.hir().def_path(def_id);

        let mir = &*self.tcx.mir_validated(def_id).0.borrow();

        let graph_path = PathBuf::from("nll-facts")
            .join(def_path.to_filename_friendly_no_crate())
            .join("graph.dot");
        let graph_file = File::create(graph_path).expect("Unable to create file");
        let graph = BufWriter::new(graph_file);

        let initialization = compute_definitely_initialized(&mir, self.tcx, def_path.clone());

        let mut mir_info_printer = MirInfoPrinter {
            def_path: def_path,
            tcx: self.tcx,
            mir: &mir,
            graph: cell::RefCell::new(graph),
            initialization: initialization,
            polonius_info: PoloniusInfo::new(self.tcx, def_id, &mir),
        };
        mir_info_printer.print_info().unwrap();

        trace!("[visit_fn] exit");
    }
}

struct MirInfoPrinter<'a, 'tcx: 'a> {
    pub def_path: rustc::hir::map::definitions::DefPath,
    pub tcx: TyCtxt<'tcx>,
    pub mir: &'a mir::Body<'tcx>,
    pub graph: cell::RefCell<BufWriter<File>>,
    pub initialization: DefinitelyInitializedAnalysisResult<'tcx>,
    pub polonius_info: PoloniusInfo,
}

macro_rules! write_graph {
    ( $self:ident, $( $x:expr ),* ) => {
        writeln!($self.graph.borrow_mut(), $( $x ),*)?;
    }
}

macro_rules! to_html {
    ( $o:expr ) => {{
        format!("{:?}", $o)
            .replace("{", "\\{")
            .replace("}", "\\}")
            .replace("&", "&amp;")
            .replace(">", "&gt;")
            .replace("<", "&lt;")
            .replace("\n", "<br/>")
    }};
}

macro_rules! write_edge {
    ( $self:ident, $source:ident, str $target:ident ) => {{
        write_graph!($self, "\"{:?}\" -> \"{}\"\n", $source, stringify!($target));
    }};
    ( $self:ident, $source:ident, unwind $target:ident ) => {{
        write_graph!($self, "\"{:?}\" -> \"{:?}\" [color=red]\n", $source, $target);
    }};
    ( $self:ident, $source:ident, imaginary $target:ident ) => {{
        write_graph!($self, "\"{:?}\" -> \"{:?}\" [style=\"dashed\"]\n", $source, $target);
    }};
    ( $self:ident, $source:ident, $target:ident ) => {{
        write_graph!($self, "\"{:?}\" -> \"{:?}\"\n", $source, $target);
    }};
}

macro_rules! to_sorted_string {
    ( $o:expr ) => {{
        let mut vector = $o.iter().map(|x| to_html!(x)).collect::<Vec<String>>();
        vector.sort();
        vector.join(", ")
    }}
}

impl<'a, 'tcx> MirInfoPrinter<'a, 'tcx> {

    pub fn print_info(&mut self) -> Result<(),io::Error> {
        write_graph!(self, "digraph G {{\n");
        for bb in self.mir.basic_blocks().indices() {
            self.visit_basic_block(bb)?;
        }
        self.print_temp_variables()?;
        write_graph!(self, "}}\n");
        Ok(())
    }

    fn print_temp_variables(&self) -> Result<(),io::Error> {
        if configuration::dump_show_temp_variables() {
            write_graph!(self, "Variables [ style=filled shape = \"record\"");
            write_graph!(self, "label =<<table>");
            write_graph!(self, "<tr><td>VARIABLES</td></tr>");
            write_graph!(self, "<tr><td>Name</td><td>Temporary</td><td>Type</td><td>Region</td></tr>");
            for (temp, var) in self.mir.local_decls.iter_enumerated() {
                // let name = var.name.map(|s| s.to_string()).unwrap_or(String::from(""));
                let name = String::from("dummy");
                let region = self.polonius_info.variable_regions
                    .get(&temp)
                    .map(|region| format!("{:?}", region))
                    .unwrap_or(String::from(""));
                let typ = to_html!(var.ty);
                write_graph!(self, "<tr><td>{}</td><td>{:?}</td><td>{}</td><td>{}</td></tr>",
                             name, temp, typ, region);
            }
            write_graph!(self, "</table>>];");
        }
        Ok(())
    }

    fn visit_basic_block(&mut self, bb: mir::BasicBlock) -> Result<(),io::Error> {
        write_graph!(self, "\"{:?}\" [ shape = \"record\"", bb);
        //if self.loops.loop_heads.contains(&bb) {
            //write_graph!(self, "color=green");
        //}
        write_graph!(self, "label =<<table>");
        write_graph!(self, "<th>");
        write_graph!(self, "<td>{:?}</td>", bb);
        write_graph!(self, "<td colspan=\"7\"></td>");
        write_graph!(self, "<td>Definitely Initialized</td>");
        write_graph!(self, "</th>");

        write_graph!(self, "<th>");
        if configuration::dump_show_statement_indices() {
            write_graph!(self, "<td>Nr</td>");
        }
        write_graph!(self, "<td>statement</td>");
        write_graph!(self, "<td colspan=\"2\">Loans</td>");
        write_graph!(self, "<td colspan=\"2\">Borrow Regions</td>");
        write_graph!(self, "<td colspan=\"2\">Regions</td>");
        write_graph!(self, "<td>{}</td>", self.get_definitely_initialized_before_block(bb));
        write_graph!(self, "</th>");

        let mir::BasicBlockData { ref statements, ref terminator, .. } = self.mir[bb];
        let mut location = mir::Location { block: bb, statement_index: 0 };
        let terminator_index = statements.len();

        while location.statement_index < terminator_index {
            self.visit_statement(location, &statements[location.statement_index])?;
            location.statement_index += 1;
        }
        let terminator = terminator.clone();
        let term_str = if let Some(ref term) = &terminator {
            let kind_str = to_html!(term.kind);
            match term.kind {
                mir::TerminatorKind::Call {
                    func: mir::Operand::Constant(
                        box mir::Constant {
                            literal: ty::Const {
                                ty: ty::TyS {
                                    kind: ty::TyKind::FnDef (def_id, substs),
                                    ..
                                },
                                ..
                            },
                            ..
                        }
                    ),
                    ..
                } => {
                    // Get the unique identifier of the defintion:
                    //let def_path = self.tcx.def_path(*def_id);
                    let def_path = self.tcx.def_path_debug_str(*def_id);
                    format!("{}<br />{}<br />{}", kind_str, to_html!(def_path), to_html!(substs))
                }
                _ => kind_str,
            }
        } else {
            String::from("")
        };
        write_graph!(self, "<tr>");
        if configuration::dump_show_statement_indices() {
            write_graph!(self, "<td></td>");
        }
        write_graph!(self, "<td>{}</td>", term_str);
        write_graph!(self, "<td></td>");
        self.write_mid_point_blas(location)?;
        write_graph!(self, "<td colspan=\"4\"></td>");
            write_graph!(self, "<td>{}</td>",
                         self.get_definitely_initialized_after_statement(location));
        write_graph!(self, "</tr>");
        write_graph!(self, "</table>> ];");

        if let Some(ref terminator) = &terminator {
            self.visit_terminator(bb, terminator)?;
        }

        Ok(())
    }

    fn visit_statement(&self, location: mir::Location,
                       statement: &mir::Statement) -> Result<(),io::Error> {
        write_graph!(self, "<tr>");
        if configuration::dump_show_statement_indices() {
            write_graph!(self, "<td>{}</td>", location.statement_index);
        }
        write_graph!(self, "<td>{}</td>", to_html!(statement));

        let start_point = self.get_point(location, facts::PointType::Start);
        let mid_point = self.get_point(location, facts::PointType::Mid);

        // Loans.
        if let Some(ref blas) = self.polonius_info.borrowck_out_facts.borrow_live_at.get(&start_point).as_ref() {
            write_graph!(self, "<td>{}</td>", to_sorted_string!(blas));
        } else {
            write_graph!(self, "<td></td>");
        }
        self.write_mid_point_blas(location)?;

        // Borrow regions (loan start points).
        let borrow_regions: Vec<_> = self.polonius_info.borrowck_in_facts
            .borrow_region
            .iter()
            .filter(|(_, _, point)| *point == start_point)
            .cloned()
            .map(|(region, loan, _)| (region, loan))
            .collect();
        write_graph!(self, "<td>{}</td>", to_sorted_string!(borrow_regions));
        let borrow_regions: Vec<_> = self.polonius_info.borrowck_in_facts
            .borrow_region
            .iter()
            .filter(|(_, _, point)| *point == mid_point)
            .cloned()
            .map(|(region, loan, _)| (region, loan))
            .collect();
        write_graph!(self, "<td>{}</td>", to_sorted_string!(borrow_regions));

        // Regions alive at this program point.
        let regions: Vec<_> = self.polonius_info.borrowck_in_facts
            .region_live_at
            .iter()
            .filter(|(_, point)| *point == start_point)
            .cloned()
            // TODO: Understand why we cannot unwrap here:
            .map(|(region, _)| (region, self.polonius_info.find_variable(region)))
            .collect();
        write_graph!(self, "<td>{}</td>", to_sorted_string!(regions));
        let regions: Vec<_> = self.polonius_info.borrowck_in_facts
            .region_live_at
            .iter()
            .filter(|(_, point)| *point == mid_point)
            .cloned()
            // TODO: Understand why we cannot unwrap here:
            .map(|(region, _)| (region, self.polonius_info.find_variable(region)))
            .collect();
        write_graph!(self, "<td>{}</td>", to_sorted_string!(regions));

        write_graph!(self, "<td>{}</td>",
                     self.get_definitely_initialized_after_statement(location));

        write_graph!(self, "</tr>");
        Ok(())
    }

    fn get_point(&self, location: mir::Location, point_type: facts::PointType) -> facts::PointIndex {
        let point = facts::Point {
            location: location,
            typ: point_type,
        };
        self.polonius_info.interner.get_point_index(&point)
    }

    /// Print the HTML cell with loans at given location.
    fn write_mid_point_blas(&self, location: mir::Location) -> Result<(),io::Error> {
        let mid_point = self.get_point(location, facts::PointType::Mid);
        let borrow_live_at_map = &self.polonius_info.borrowck_out_facts.borrow_live_at;
        let mut blas = if let Some(ref blas) = borrow_live_at_map.get(&mid_point).as_ref() {
            (**blas).clone()
        } else {
            Vec::new()
        };

        // Format the loans and mark the dying ones.
        blas.sort();

        write_graph!(self, "<td>{}</td>", to_sorted_string!(blas));

        Ok(())
    }

    fn visit_terminator(&self, bb: mir::BasicBlock, terminator: &mir::Terminator) -> Result<(),io::Error> {
        use rustc::mir::TerminatorKind;
        match terminator.kind {
            TerminatorKind::Goto { target } => {
                write_edge!(self, bb, target);
            }
            TerminatorKind::SwitchInt { ref targets, .. } => {
                for target in targets {
                    write_edge!(self, bb, target);
                }
            }
            TerminatorKind::Resume => {
                write_edge!(self, bb, str resume);
            }
            TerminatorKind::Abort => {
                write_edge!(self, bb, str abort);
            }
            TerminatorKind::Return => {
                write_edge!(self, bb, str return);
            }
            TerminatorKind::Unreachable => {}
            TerminatorKind::DropAndReplace { ref target, unwind, .. } |
            TerminatorKind::Drop { ref target, unwind, .. } => {
                write_edge!(self, bb, target);
                if let Some(target) = unwind {
                    write_edge!(self, bb, unwind target);
                }
            }
            TerminatorKind::Call { ref destination, cleanup, .. } => {
                if let &Some((_, target)) = destination {
                    write_edge!(self, bb, target);
                }
                if let Some(target) = cleanup {
                    write_edge!(self, bb, unwind target);
                }
            }
            TerminatorKind::Assert { target, cleanup, .. } => {
                write_edge!(self, bb, target);
                if let Some(target) = cleanup {
                    write_edge!(self, bb, unwind target);
                }
            }
            TerminatorKind::Yield { .. } => { unimplemented!() }
            TerminatorKind::GeneratorDrop => { unimplemented!() }
            TerminatorKind::FalseEdges { ref real_target, ref imaginary_target } => {
                write_edge!(self, bb, real_target);
                write_edge!(self, bb, imaginary_target);
            }
            TerminatorKind::FalseUnwind { real_target, unwind } => {
                write_edge!(self, bb, real_target);
                if let Some(target) = unwind {
                    write_edge!(self, bb, imaginary target);
                }
            }
        };
        Ok(())
    }
}

/// Definitely initialized analysis.
impl<'a, 'tcx> MirInfoPrinter<'a, 'tcx> {

    fn get_definitely_initialized_before_block(&self, bb: mir::BasicBlock) -> String {
        let place_set = self.initialization.get_before_block(bb);
        to_sorted_string!(place_set)
    }


    fn get_definitely_initialized_after_statement(&self, location: mir::Location) -> String {
        let place_set = self.initialization.get_after_statement(location);
        to_sorted_string!(place_set)
    }
}
