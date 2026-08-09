pub mod direct;

use std::{fmt::Display, sync::Arc};

use super::compile_graph::CompileGraph;
use super::task_monitor::TaskMonitor;
use super::CompilerOptions;
use enum_dispatch::enum_dispatch;
use mchprs_blocks::BlockPos;
use mchprs_world::{TickEntry, World};

#[derive(Debug)]
pub enum UseBlockError {
    NotFound,
    NotSupported,
    Useless
}
impl Display for UseBlockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UseBlockError::NotFound => f.write_str("Block not found in backend"),
            UseBlockError::NotSupported => f.write_str("Opperation on block is not supported"),
            UseBlockError::Useless => f.write_str("Action does not have any observable effects")
        }
    }
}

#[enum_dispatch]
pub trait JITBackend {
    fn compile(
        &mut self,
        graph: CompileGraph,
        ticks: Vec<TickEntry>,
        options: &CompilerOptions,
        monitor: Arc<TaskMonitor>,
    );
    fn tick(&mut self);

    fn tickn(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.tick();
        }
    }

    fn on_use_block(&mut self, pos: BlockPos) -> Result<(), UseBlockError>;
    fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) -> Result<(), UseBlockError>;
    fn flush<W: World>(&mut self, world: &mut W, io_only: bool);
    fn reset<W: World>(&mut self, world: &mut W, io_only: bool);
    fn has_pending_ticks(&self) -> bool;
    /// Inspect block for debugging
    fn inspect(&mut self, pos: BlockPos);
}

use direct::DirectBackend;

#[enum_dispatch(JITBackend)]
pub enum BackendDispatcher {
    DirectBackend,
}
