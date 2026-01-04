use std::collections::HashMap;
use std::ops::Deref;
use std::time::Instant;

use super::Pass;
use crate::compile_graph::{CompileGraph, CompileLink, CompileNode, NodeIdx};
use crate::passes::{AnalysisInfo, AnalysisInfos};
use crate::{CompilerInput, CompilerOptions};
use itertools::Itertools;
use mchprs_world::World;
use petgraph::Direction::{Incoming, Outgoing};
use petgraph::graph::EdgeIndex;
use petgraph::visit::{EdgeIndexable, EdgeRef, NodeIndexable};
use rustc_hash::FxHashMap;



pub struct PartitioningInfo {
    partitions: Vec<u32>,
}

impl AnalysisInfo for PartitioningInfo {}

impl PartitioningInfo {
    pub fn get(&self, node: NodeIdx) -> u32 {
        self.partitions[node.index()]
    }
    
    pub fn outputs(&self, partition: usize) -> &[usize] {
        match partition {
            1 => &[2],
            _ => &[]
        }
    }

    pub fn inputs(&self, partition: usize) -> &[usize] {
        match partition {
            2 => &[1],
            _ => &[]
        }
    }
}

pub struct PartitionGraph;

impl<W: World> Pass<W> for PartitionGraph {
    fn run_pass(
        &self,
        graph: &mut CompileGraph,
        _: &CompilerOptions,
        _: &CompilerInput<'_, W>,
        analysis_infos: &mut AnalysisInfos,
    ) {
        let t0 = Instant::now(); 
        let mut components = Vec::new();
        let mut tarjan = petgraph::algo::TarjanScc::new();
        tarjan.run(graph.deref(), |component| {
            components.push(component.to_vec());
        });
        let t1 = Instant::now();

        
        let biggest = components.iter().position_max_by_key(|c| c.len()).unwrap_or(0);
        
        let mut partitions = vec![0; graph.node_bound()];

        let biggest_partition = 1;
        
        for idx in components[biggest].iter().copied() {
            partitions[idx.index()] = biggest_partition;
        }

        // let partition_count = 2;
        let output_partition = 2;
        // let input_partition = 3;
        // let mut output_cnt = 0;
        // let mut input_cnt = 0;

        // let mut running = true;
        // while running {
        //     running = false;
        //     for idx in graph.node_indices() {
        //         if partitions[idx.index()] == biggest_partition {
        //             continue;
        //         }
        //         let mut total = 0;
        //         let mut big = 0;
        //         for input in graph.neighbors_directed(idx, Incoming) {
        //             total += 1;
        //             if partitions[input.index()] == biggest_partition {
        //                 big += 1;
        //             }
        //         }
        //         if big == 0 || big == total {
        //             continue;
        //         }
        //         running = true;
        //         partitions[idx.index()] = biggest_partition;
        //         components[biggest].push(idx);
        //     }
        // }

        for idx in components[biggest].iter().copied() {
            let mut stack = vec![idx];
            while let Some(idx) = stack.pop() {
                for input in graph.neighbors_directed(idx, Outgoing) {
                    if partitions[input.index()] != 0 {
                        continue;
                    }
                    partitions[input.index()] = biggest_partition;
                    stack.push(input);
                    // input_cnt += 1;
                }
            }
        }

        // for idx in graph.node_indices() {
        //     // if partitions[idx.index()] == {
        //     //     continue;
        //     // }
        //     let mut stack = vec![idx];
        //     while let Some(idx) = stack.pop() {
        //         for output in graph.neighbors_directed(idx, Outgoing) {
        //             if partitions[output.index()] != 0 {
        //                 continue;
        //             }
        //             partitions[output.index()] = output_partition;
        //             stack.push(output);
        //             // output_cnt += 1;
        //         }
        //     }
        // }

        for idx in graph.node_indices() {
            if partitions[idx.index()] != 0 {
                continue;
            }
            partitions[idx.index()] = biggest_partition;
        }

        for idx in graph.node_indices() {
            if partitions[idx.index()] != output_partition {
                continue;
            }
            for output in graph.neighbors_directed(idx, Outgoing) {
                assert_eq!(partitions[output.index()], output_partition);
            }

        }

        // println!("{:?} {} {} {}", t1 - t0, components[biggest].len(), output_cnt, input_cnt);
        assert!(graph.node_indices().all(|idx| {
            let p = partitions[idx.index()];
            // if p == biggest_partition {
            //     assert!(!graph[idx].is_output);
            // }

            p == biggest_partition || p == output_partition
        } ));

        analysis_infos.insert_analysis(PartitioningInfo {
            partitions
        });

    }

    fn status_message(&self) -> &'static str {
        "Clamping weights"
    }
}
