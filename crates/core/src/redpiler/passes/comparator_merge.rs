//! # [`TailCoalesce`]
//!
//! Turns all lines of comparators that only end in outputs into `Buffer` components.
//! The first comparator in the line is kept as is, to guarded the Buffer from non-`Normal` priority updates.

use super::Pass;
use crate::redpiler::compile_graph::{
    Annotations, CompileGraph, CompileLink, CompileNode, LinkType, NodeIdx, NodeType,
};
use crate::redpiler::{BackendVariant, CompilerInput, CompilerOptions};
use crate::world::World;
use itertools::Itertools;
use petgraph::Direction;
use rustc_hash::FxHashSet;

pub struct ComparatorMerge;

impl<W: World> Pass<W> for ComparatorMerge {
    fn run_pass(&self, graph: &mut CompileGraph, _: &CompilerOptions, _: &CompilerInput<'_, W>) {
        // Identify all lines
        let comparator_lines = find_lines(graph, |idx| {
            matches!(
                graph[idx].ty,
                NodeType::Comparator {
                    far_input: None,
                    facing_diode: false,
                    ..
                }
            )
        });

        let repeater_lines = find_lines(graph, |idx| {
            let node = &graph[idx];
            matches!(node.ty, NodeType::Repeater { .. })
        });

        let histogram = repeater_lines.iter().counts_by(|line| line.len());
        println!(
            "repeater_lines: {:?}",
            histogram.iter().sorted().collect_vec()
        );

        let histogram = comparator_lines.iter().counts_by(|line| line.len());
        println!(
            "ComparatorMerge: {:?}",
            histogram.iter().sorted().collect_vec()
        );

        // Replace all valid identified lines with special line nodes
        for line in comparator_lines
            .iter()
            .flat_map(|line| line.chunks(64))
            .filter(|line| line.len() >= 4)
        {
            // Merge signal strength falloff
            let falloff: usize = line
                .windows(2)
                .map(|n| graph.find_edge(n[0], n[1]).unwrap())
                .map(|idx| graph[idx].ss as usize)
                .sum();

            if falloff < 15 {
                let start = line[0];
                let end = line[line.len() - 1];

                let states = line
                    .iter()
                    .map(|&idx| graph[idx].state.output_strength)
                    .collect_vec();

                let node = CompileNode {
                    ty: NodeType::ComparatorLine {
                        states: states.into_boxed_slice(),
                    },
                    block: None,
                    state: graph[end].state.clone(),
                    is_input: false,
                    is_output: false,
                    annotations: Annotations::default(),
                };
                let idx = graph.add_node(node);

                let mut incomming = graph
                    .neighbors_directed(start, Direction::Incoming)
                    .detach();
                while let Some(edge_idx) = incomming.next_edge(graph) {
                    let source = graph.edge_endpoints(edge_idx).unwrap().0;
                    let link = graph.remove_edge(edge_idx).unwrap();
                    let ss = falloff as u8 + link.ss;
                    if ss < 15 {
                        graph.add_edge(source, idx, CompileLink::default(ss));
                    }
                }
                let mut outgoing = graph.neighbors_directed(end, Direction::Outgoing).detach();
                while let Some(edge_idx) = outgoing.next_edge(graph) {
                    let target = graph.edge_endpoints(edge_idx).unwrap().1;
                    let link = graph.remove_edge(edge_idx).unwrap();
                    graph.add_edge(idx, target, link);
                }
            }

            for &idx in line {
                graph.remove_node(idx);
            }
        }
    }

    fn should_run(&self, options: &CompilerOptions) -> bool {
        options.optimize && options.backend_variant == BackendVariant::Threading
    }
    
    fn status_message(&self) -> &'static str {
        "merging comparators"
    }
}

/// Finds all lines of components that match the specified predicate.
/// Starts at some node idx and then matches for more line nodes in both directions.
fn find_lines<F>(graph: &CompileGraph, predicate: F) -> Vec<Vec<NodeIdx>>
where
    F: Fn(NodeIdx) -> bool,
{
    let mut visited = FxHashSet::default();

    let mut lines = vec![];
    for idx in graph.node_indices() {
        // Check if valid starting point
        if visited.contains(&idx) || !predicate(idx) || !is_line(graph, idx, false, false) {
            continue;
        }

        let mut line = vec![idx];
        visited.insert(idx);

        // Match for line components backwards
        let mut cur = next(graph, idx, Direction::Incoming);
        while !visited.contains(&cur) && predicate(cur) && is_line(graph, cur, false, false) {
            line.push(cur);
            visited.insert(cur);
            cur = next(graph, cur, Direction::Incoming);
        }
        // Add head (may have multiple inputs)
        if !visited.contains(&cur) && predicate(cur) && is_line(graph, cur, true, false) {
            line.push(cur);
            visited.insert(cur);
        }
        line.reverse();

        // And then match forward
        let mut cur = next(graph, idx, Direction::Outgoing);
        while !visited.contains(&cur) && predicate(cur) && is_line(graph, cur, false, false) {
            line.push(cur);
            visited.insert(cur);
            cur = next(graph, cur, Direction::Outgoing);
        }
        // Add tail (may have multiple outputs)
        if !visited.contains(&cur) && predicate(cur) && is_line(graph, cur, false, true) {
            line.push(cur);
            visited.insert(cur);
        }

        lines.push(line);
    }
    assert!(lines.iter().flatten().all_unique());
    return lines;
}

/// Checks is a node could be safely removed and is part of a line.
/// Lines have exactly one incomming default edge or multiple if `any_input`
/// and have exactly one outgoing default/side edge or multiple if `any_output`
fn is_line(graph: &CompileGraph, idx: NodeIdx, any_input: bool, any_output: bool) -> bool {
    let mut incomming = graph.edges_directed(idx, Direction::Incoming);
    let input_valid = if any_input {
        incomming.all(|e| e.weight().ty == LinkType::Default)
    } else {
        incomming
            .exactly_one()
            .is_ok_and(|e| e.weight().ty == LinkType::Default)
    };

    let outgoing = graph.edges_directed(idx, Direction::Outgoing);
    let output_valid = any_output || outgoing.exactly_one().is_ok();

    graph[idx].is_removable() && input_valid && output_valid
}

fn next(graph: &CompileGraph, idx: NodeIdx, dir: Direction) -> NodeIdx {
    graph.neighbors_directed(idx, dir).exactly_one().unwrap()
}
