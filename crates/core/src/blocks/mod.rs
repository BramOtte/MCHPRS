mod redstone;

use crate::items::{ActionResult, UseOnBlockContext};
use crate::player::Player;
use crate::world::World;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::items::Item;
use mchprs_blocks::{
    BlockColorVariant, BlockDirection, BlockFace, BlockFacing, BlockPos, BlockProperty,
};
use mchprs_proc_macros::BlockTransform;
use mchprs_world::TickPriority;
pub use redstone::*;
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Clone, Copy, Debug)]
pub enum FlipDirection {
    FlipX,
    FlipZ,
}

#[derive(Clone, Copy, Debug)]
pub enum RotateAmt {
    Rotate90,
    Rotate180,
    Rotate270,
}

trait BlockTransform {
    fn rotate(&mut self, amt: crate::blocks::RotateAmt) {
        match amt {
            // ez
            RotateAmt::Rotate90 => self.rotate90(),
            RotateAmt::Rotate180 => {
                self.rotate90();
                self.rotate90();
            }
            RotateAmt::Rotate270 => {
                self.rotate90();
                self.rotate90();
                self.rotate90();
            }
        }
    }
    fn rotate90(&mut self);
    fn flip(&mut self, dir: crate::blocks::FlipDirection);
}

impl<T> BlockTransform for T {
    default fn rotate90(&mut self) {}
    default fn flip(&mut self, _dir: crate::blocks::FlipDirection) {}
}

impl BlockTransform for BlockDirection {
    fn flip(&mut self, dir: FlipDirection) {
        match dir {
            FlipDirection::FlipX => match self {
                BlockDirection::East => *self = BlockDirection::West,
                BlockDirection::West => *self = BlockDirection::East,
                _ => {}
            },
            FlipDirection::FlipZ => match self {
                BlockDirection::North => *self = BlockDirection::South,
                BlockDirection::South => *self = BlockDirection::North,
                _ => {}
            },
        }
    }

    fn rotate90(&mut self) {
        *self = match self {
            BlockDirection::North => BlockDirection::East,
            BlockDirection::East => BlockDirection::South,
            BlockDirection::South => BlockDirection::West,
            BlockDirection::West => BlockDirection::North,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrapdoorHalf {
    Top,
    Bottom,
}

impl TrapdoorHalf {
    pub fn get_id(self) -> u32 {
        self as u32
    }

    pub fn from_id(id: u32) -> TrapdoorHalf {
        use TrapdoorHalf::*;
        match id {
            0 => Top,
            1 => Bottom,
            _ => unreachable!(),
        }
    }
}

impl ToString for TrapdoorHalf {
    fn to_string(&self) -> String {
        match self {
            TrapdoorHalf::Top => "top".to_owned(),
            TrapdoorHalf::Bottom => "bottom".to_owned(),
        }
    }
}

impl FromStr for TrapdoorHalf {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "top" => TrapdoorHalf::Top,
            "bottom" => TrapdoorHalf::Bottom,
            _ => return Err(()),
        })
    }
}

impl Block {
    pub fn has_block_entity(self) -> bool {
        matches!(
            self,
            Block::RedstoneComparator { .. }
                | Block::Barrel { .. }
                | Block::Furnace { .. }
                | Block::Hopper { .. }
                | Block::Sign { .. }
                | Block::WallSign { .. }
        )
    }

    pub fn has_comparator_override(self) -> bool {
        matches!(
            self,
            Block::Barrel { .. }
                | Block::Furnace { .. }
                | Block::Hopper { .. }
                | Block::Cauldron { .. }
                | Block::Composter { .. }
        )
    }

    pub fn get_comparator_override(self, world: &impl World, pos: BlockPos) -> u8 {
        match self {
            Block::Barrel { .. } | Block::Furnace { .. } | Block::Hopper { .. } => {
                if let Some(BlockEntity::Container {
                    comparator_override,
                    ..
                }) = world.get_block_entity(pos)
                {
                    *comparator_override
                } else {
                    0
                }
            }
            Block::Cauldron { level } => level,
            Block::Composter { level } => level,
            _ => 0,
        }
    }

    pub fn is_diode(self) -> bool {
        matches!(
            self,
            Block::RedstoneRepeater { .. } | Block::RedstoneComparator { .. }
        )
    }

    pub fn can_place_block_in(self) -> bool {
        matches!(self.get_id(),
            0             // Air
            | 9915..=9916 // Void and Cave air
            | 34..=49     // Water
            | 50..=65     // Lava
            | 1398        // Grass
            | 1399        // Fern
            | 1400        // Dead bush
            | 1401        // Seagrass
            | 1402..=1403 // Tall Seagrass
            | 8143..=8144 // Tall Grass
            | 8145..=8146 // Tall Fern
        )
    }

    pub fn on_use(
        self,
        world: &mut impl World,
        player: &mut Player,
        pos: BlockPos,
        item_in_hand: Option<Item>,
    ) -> ActionResult {
        match self {
            Block::RedstoneRepeater { repeater } => {
                let mut repeater = repeater;
                repeater.delay += 1;
                if repeater.delay > 4 {
                    repeater.delay -= 4;
                }
                world.set_block(pos, Block::RedstoneRepeater { repeater });
                ActionResult::Success
            }
            Block::RedstoneComparator { comparator } => {
                let mut comparator = comparator;
                comparator.mode = comparator.mode.toggle();
                comparator.tick(world, pos);
                world.set_block(pos, Block::RedstoneComparator { comparator });
                ActionResult::Success
            }
            Block::Lever { mut lever } => {
                lever.powered = !lever.powered;
                world.set_block(pos, Block::Lever { lever });
                Block::update_surrounding_blocks(world, pos);
                match lever.face {
                    LeverFace::Ceiling => {
                        Block::update_surrounding_blocks(world, pos.offset(BlockFace::Top));
                    }
                    LeverFace::Floor => {
                        Block::update_surrounding_blocks(world, pos.offset(BlockFace::Bottom));
                    }
                    LeverFace::Wall => Block::update_surrounding_blocks(
                        world,
                        pos.offset(lever.facing.opposite().block_face()),
                    ),
                }
                ActionResult::Success
            }
            Block::StoneButton { mut button } => {
                if !button.powered {
                    button.powered = true;
                    world.set_block(pos, Block::StoneButton { button });
                    world.schedule_tick(pos, 10, TickPriority::Normal);
                    Block::update_surrounding_blocks(world, pos);
                    match button.face {
                        ButtonFace::Ceiling => {
                            Block::update_surrounding_blocks(world, pos.offset(BlockFace::Top));
                        }
                        ButtonFace::Floor => {
                            Block::update_surrounding_blocks(world, pos.offset(BlockFace::Bottom));
                        }
                        ButtonFace::Wall => Block::update_surrounding_blocks(
                            world,
                            pos.offset(button.facing.opposite().block_face()),
                        ),
                    }
                }
                ActionResult::Success
            }
            Block::RedstoneWire { wire } => wire.on_use(world, pos),
            Block::SeaPickle { pickles } => {
                if let Some(Item::SeaPickle {}) = item_in_hand {
                    if pickles < 4 {
                        world.set_block(
                            pos,
                            Block::SeaPickle {
                                pickles: pickles + 1,
                            },
                        );
                    }
                }
                ActionResult::Success
            }
            b if b.has_comparator_override() => {
                // Open container
                // TODO: Avoid clone
                let block_entity = world.get_block_entity(pos).cloned();
                if let Some(BlockEntity::Container { inventory, ty, .. }) = block_entity {
                    player.open_container(&inventory, ty);
                }
                ActionResult::Success
            }
            _ => ActionResult::Pass,
        }
    }

    pub fn get_state_for_placement(
        world: &impl World,
        pos: BlockPos,
        item: Item,
        context: &UseOnBlockContext,
    ) -> Block {
        let block = match item {
            Item::Stone {} => Block::Stone {},
            Item::Glass {} => Block::Glass {},
            Item::Sandstone {} => Block::Sandstone {},
            Item::SeaPickle {} => Block::SeaPickle { pickles: 1 },
            Item::Wool { color } => Block::Wool { color },
            Item::Furnace {} => Block::Furnace {},
            Item::StonePressurePlate {} => Block::StonePressurePlate { powered: false },
            Item::Lever {} => {
                let lever_face = match context.block_face {
                    BlockFace::Top => LeverFace::Floor,
                    BlockFace::Bottom => LeverFace::Ceiling,
                    _ => LeverFace::Wall,
                };
                let facing = if lever_face == LeverFace::Wall {
                    context.block_face.to_direction()
                } else {
                    context.player_direction
                };
                Block::Lever {
                    lever: Lever::new(lever_face, facing, false),
                }
            }
            Item::RedstoneTorch {} => match context.block_face {
                BlockFace::Top | BlockFace::Bottom => Block::RedstoneTorch { lit: true },
                face => Block::RedstoneWallTorch {
                    lit: true,
                    facing: face.to_direction(),
                },
            },
            Item::TripwireHook {} => match context.block_face {
                BlockFace::Bottom | BlockFace::Top => Block::Air {},
                direction => Block::TripwireHook {
                    direction: direction.to_direction(),
                },
            },
            Item::StoneButton {} => {
                let button_face = match context.block_face {
                    BlockFace::Top => ButtonFace::Floor,
                    BlockFace::Bottom => ButtonFace::Ceiling,
                    _ => ButtonFace::Wall,
                };
                let facing = if button_face == ButtonFace::Wall {
                    context.block_face.to_direction()
                } else {
                    context.player_direction
                };
                Block::StoneButton {
                    button: StoneButton::new(button_face, facing, false),
                }
            }
            Item::RedstoneLamp {} => Block::RedstoneLamp {
                lit: Block::redstone_lamp_should_be_lit(world, pos),
            },
            Item::RedstoneBlock {} => Block::RedstoneBlock {},
            Item::Hopper {} => Block::Hopper {},
            Item::Terracotta {} => Block::Terracotta {},
            Item::ColoredTerracotta { color } => Block::ColoredTerracotta { color },
            Item::Concrete { color } => Block::Concrete { color },
            Item::Repeater {} => Block::RedstoneRepeater {
                repeater: RedstoneRepeater::get_state_for_placement(
                    world,
                    pos,
                    context.player_direction.opposite(),
                ),
            },
            Item::Comparator {} => Block::RedstoneComparator {
                comparator: RedstoneComparator::new(
                    context.player_direction.opposite(),
                    ComparatorMode::Compare,
                    false,
                ),
            },
            Item::Sign { sign_type } => match context.block_face {
                BlockFace::Bottom => Block::Air {},
                BlockFace::Top => Block::Sign {
                    sign_type,
                    rotation: (((180.0 + context.player_yaw) * 16.0 / 360.0) + 0.5).floor() as u32
                        & 15,
                },
                _ => Block::WallSign {
                    sign_type,
                    facing: context.block_face.to_direction(),
                },
            },
            Item::Redstone {} => Block::RedstoneWire {
                wire: RedstoneWire::get_state_for_placement(world, pos),
            },
            Item::Barrel {} => Block::Barrel {},
            Item::Target {} => Block::Target {},
            Item::StainedGlass { color } => Block::StainedGlass { color },
            Item::SmoothStoneSlab {} => Block::SmoothStoneSlab {},
            Item::QuartzSlab {} => Block::QuartzSlab {},
            _ => Block::Air {},
        };
        if block.is_valid_position(world, pos) {
            block
        } else {
            Block::Air {}
        }
    }

    pub fn place_in_world(self, world: &mut impl World, pos: BlockPos, nbt: &Option<nbt::Blob>) {
        if self.has_block_entity() {
            if let Some(nbt) = nbt {
                if let nbt::Value::Compound(compound) = &nbt["BlockEntityTag"] {
                    if let Some(block_entity) = BlockEntity::from_nbt(compound) {
                        world.set_block_entity(pos, block_entity);
                    }
                }
            };
        }
        match self {
            Block::RedstoneRepeater { .. } => {
                // TODO: Queue repeater tick
                world.set_block(pos, self);
                Block::change_surrounding_blocks(world, pos);
                Block::update_surrounding_blocks(world, pos);
            }
            Block::RedstoneWire { .. } => {
                world.set_block(pos, self);
                Block::change_surrounding_blocks(world, pos);
                Block::update_wire_neighbors(world, pos);
            }
            _ => {
                world.set_block(pos, self);
                Block::change_surrounding_blocks(world, pos);
                Block::update_surrounding_blocks(world, pos);
            }
        }
    }

    pub fn destroy(self, world: &mut impl World, pos: BlockPos) {
        if self.has_block_entity() {
            world.delete_block_entity(pos);
        }

        match self {
            Block::RedstoneWire { .. } => {
                world.set_block(pos, Block::Air {});
                Block::change_surrounding_blocks(world, pos);
                Block::update_wire_neighbors(world, pos);
            }
            Block::Lever { lever } => {
                world.set_block(pos, Block::Air {});
                // This is a horrible idea, don't do this.
                // One day this will be fixed, but for now... too bad!
                match lever.face {
                    LeverFace::Ceiling => {
                        Block::change_surrounding_blocks(world, pos.offset(BlockFace::Top));
                        Block::update_surrounding_blocks(world, pos.offset(BlockFace::Top));
                    }
                    LeverFace::Floor => {
                        Block::change_surrounding_blocks(world, pos.offset(BlockFace::Bottom));
                        Block::update_surrounding_blocks(world, pos.offset(BlockFace::Bottom));
                    }
                    LeverFace::Wall => {
                        Block::change_surrounding_blocks(
                            world,
                            pos.offset(lever.facing.opposite().block_face()),
                        );
                        Block::update_surrounding_blocks(
                            world,
                            pos.offset(lever.facing.opposite().block_face()),
                        );
                    }
                }
            }
            _ => {
                world.set_block(pos, Block::Air {});
                Block::change_surrounding_blocks(world, pos);
                Block::update_surrounding_blocks(world, pos);
            }
        }
    }

    pub fn update(self, world: &mut impl World, pos: BlockPos) {
        match self {
            Block::RedstoneWire { wire } => {
                wire.on_neighbor_updated(world, pos);
            }
            Block::RedstoneTorch { lit } => {
                if lit == Block::torch_should_be_off(world, pos) && !world.pending_tick_at(pos) {
                    world.schedule_tick(pos, 1, TickPriority::Normal);
                }
            }
            Block::RedstoneWallTorch { lit, facing } => {
                if lit == Block::wall_torch_should_be_off(world, pos, facing)
                    && !world.pending_tick_at(pos)
                {
                    world.schedule_tick(pos, 1, TickPriority::Normal);
                }
            }
            Block::RedstoneRepeater { repeater } => {
                repeater.on_neighbor_updated(world, pos);
            }
            Block::RedstoneComparator { comparator } => {
                comparator.update(world, pos);
            }
            Block::RedstoneLamp { lit } => {
                let should_be_lit = Block::redstone_lamp_should_be_lit(world, pos);
                if lit && !should_be_lit {
                    world.schedule_tick(pos, 2, TickPriority::Normal);
                } else if !lit && should_be_lit {
                    world.set_block(pos, Block::RedstoneLamp { lit: true });
                }
            }
            Block::IronTrapdoor {
                powered,
                facing,
                half,
            } => {
                let should_be_powered = Block::redstone_lamp_should_be_lit(world, pos);
                if powered != should_be_powered {
                    let new_block = Block::IronTrapdoor {
                        facing,
                        half,
                        powered: should_be_powered,
                    };
                    world.set_block(pos, new_block);
                }
            }
            _ => {}
        }
    }

    pub fn tick(self, world: &mut impl World, pos: BlockPos) {
        match self {
            Block::RedstoneRepeater { repeater } => {
                repeater.tick(world, pos);
            }
            Block::RedstoneComparator { comparator } => {
                comparator.tick(world, pos);
            }
            Block::RedstoneTorch { lit } => {
                let should_be_off = Block::torch_should_be_off(world, pos);
                if lit && should_be_off {
                    world.set_block(pos, Block::RedstoneTorch { lit: false });
                    Block::update_surrounding_blocks(world, pos);
                } else if !lit && !should_be_off {
                    world.set_block(pos, Block::RedstoneTorch { lit: true });
                    Block::update_surrounding_blocks(world, pos);
                }
            }
            Block::RedstoneWallTorch { lit, facing } => {
                let should_be_off = Block::wall_torch_should_be_off(world, pos, facing);
                if lit && should_be_off {
                    world.set_block(pos, Block::RedstoneWallTorch { lit: false, facing });
                    Block::update_surrounding_blocks(world, pos);
                } else if !lit && !should_be_off {
                    world.set_block(pos, Block::RedstoneWallTorch { lit: true, facing });
                    Block::update_surrounding_blocks(world, pos);
                }
            }
            Block::RedstoneLamp { lit } => {
                let should_be_lit = Block::redstone_lamp_should_be_lit(world, pos);
                if lit && !should_be_lit {
                    world.set_block(pos, Block::RedstoneLamp { lit: false });
                }
            }
            Block::StoneButton { mut button } => {
                if button.powered {
                    button.powered = false;
                    world.set_block(pos, Block::StoneButton { button });
                    Block::update_surrounding_blocks(world, pos);
                    match button.face {
                        ButtonFace::Ceiling => {
                            Block::update_surrounding_blocks(world, pos.offset(BlockFace::Top));
                        }
                        ButtonFace::Floor => {
                            Block::update_surrounding_blocks(world, pos.offset(BlockFace::Bottom));
                        }
                        ButtonFace::Wall => Block::update_surrounding_blocks(
                            world,
                            pos.offset(button.facing.opposite().block_face()),
                        ),
                    }
                }
            }
            _ => {}
        }
    }

    pub fn is_valid_position(self, world: &impl World, pos: BlockPos) -> bool {
        if world.is_cursed() {
            return true;
        }

        match self {
            Block::RedstoneWire { .. }
            | Block::RedstoneComparator { .. }
            | Block::RedstoneRepeater { .. }
            | Block::Sign { .. }
            | Block::RedstoneTorch { .. } => {
                let bottom_block = world.get_block(pos.offset(BlockFace::Bottom));
                bottom_block.is_cube()
            }
            Block::RedstoneWallTorch { facing, .. } | Block::WallSign { facing, .. } => {
                let parent_block = world.get_block(pos.offset(facing.opposite().block_face()));
                parent_block.is_cube()
            }
            Block::TripwireHook { direction, .. } => {
                let parent_block = world.get_block(pos.offset(direction.opposite().block_face()));
                parent_block.is_cube()
            }
            Block::Lever { lever } => match lever.face {
                LeverFace::Floor => {
                    let bottom_block = world.get_block(pos.offset(BlockFace::Bottom));
                    bottom_block.is_cube()
                }
                LeverFace::Ceiling => {
                    let top_block = world.get_block(pos.offset(BlockFace::Top));
                    top_block.is_cube()
                }
                LeverFace::Wall => {
                    let parent_block =
                        world.get_block(pos.offset(lever.facing.opposite().block_face()));
                    parent_block.is_cube()
                }
            },
            Block::StoneButton { button } => match button.face {
                ButtonFace::Floor => {
                    let bottom_block = world.get_block(pos.offset(BlockFace::Bottom));
                    bottom_block.is_cube()
                }
                ButtonFace::Ceiling => {
                    let top_block = world.get_block(pos.offset(BlockFace::Top));
                    top_block.is_cube()
                }
                ButtonFace::Wall => {
                    let parent_block =
                        world.get_block(pos.offset(button.facing.opposite().block_face()));
                    parent_block.is_cube()
                }
            },
            _ => true,
        }
    }

    pub fn change(self, world: &mut impl World, pos: BlockPos, direction: BlockFace) {
        if !self.is_valid_position(world, pos) {
            self.destroy(world, pos);
            return;
        }
        if let Block::RedstoneWire { wire } = self {
            let new_state = wire.on_neighbor_changed(world, pos, direction);
            if world.set_block(pos, Block::RedstoneWire { wire: new_state }) {
                Block::update_wire_neighbors(world, pos);
            }
        }
    }

    fn update_wire_neighbors(world: &mut impl World, pos: BlockPos) {
        for direction in &BlockFace::values() {
            let neighbor_pos = pos.offset(*direction);
            let block = world.get_block(neighbor_pos);
            block.update(world, neighbor_pos);
            for n_direction in &BlockFace::values() {
                let n_neighbor_pos = neighbor_pos.offset(*n_direction);
                let block = world.get_block(n_neighbor_pos);
                block.update(world, n_neighbor_pos);
            }
        }
    }

    pub fn update_surrounding_blocks(world: &mut impl World, pos: BlockPos) {
        for direction in &BlockFace::values() {
            let neighbor_pos = pos.offset(*direction);
            let block = world.get_block(neighbor_pos);
            block.update(world, neighbor_pos);

            // Also update diagonal blocks

            let up_pos = neighbor_pos.offset(BlockFace::Top);
            let up_block = world.get_block(up_pos);
            up_block.update(world, up_pos);

            let down_pos = neighbor_pos.offset(BlockFace::Bottom);
            let down_block = world.get_block(down_pos);
            down_block.update(world, down_pos);
        }
    }

    fn change_surrounding_blocks(world: &mut impl World, pos: BlockPos) {
        for direction in &BlockFace::values() {
            let neighbor_pos = pos.offset(*direction);
            let block = world.get_block(neighbor_pos);
            block.change(world, neighbor_pos, *direction);

            // Also change diagonal blocks

            let up_pos = neighbor_pos.offset(BlockFace::Top);
            let up_block = world.get_block(up_pos);
            up_block.change(world, up_pos, *direction);

            let down_pos = neighbor_pos.offset(BlockFace::Bottom);
            let down_block = world.get_block(down_pos);
            down_block.change(world, down_pos, *direction);
        }
    }
}

#[test]
fn repeater_id_test() {
    let original = Block::RedstoneRepeater {
        repeater: RedstoneRepeater::new(3, BlockDirection::West, true, false),
    };
    let id = original.get_id();
    assert_eq!(id, 4141);
    let new = Block::from_id(id);
    assert_eq!(new, original);
}

#[test]
fn comparator_id_test() {
    let original = Block::RedstoneComparator {
        comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Subtract, false),
    };
    let id = original.get_id();
    assert_eq!(id, 6895);
    let new = Block::from_id(id);
    assert_eq!(new, original);
}

macro_rules! blocks {
    (
        $(
            $name:ident {
                props: {
                    $(
                        $prop_name:ident : $prop_type:ident
                    ),*
                },
                get_id: $get_id:expr,
                $( from_id_offset: $get_id_offset:literal, )?
                from_id($id_name:ident): $from_id_pat:pat => {
                    $(
                        $from_id_pkey:ident: $from_id_pval:expr
                    ),*
                },
                from_names($name_name:ident): {
                    $(
                        $from_name_pat:pat => {
                            $(
                                $from_name_pkey:ident: $from_name_pval:expr
                            ),*
                        }
                    ),*
                },
                get_name: $get_name:expr,
                $( solid: $solid:literal, )?
                $( transparent: $transparent:literal, )?
                $( cube: $cube:literal, )?
            }
        ),*
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Block {
            $(
                $name {
                    $(
                        $prop_name: $prop_type,
                    )*
                }
            ),*
        }

        #[allow(clippy::redundant_field_names)]
        impl Block {
            pub fn is_solid(self) -> bool {
                match self {
                    $(
                        $( Block::$name { .. } => $solid, )?
                    )*
                    _ => false
                }
            }

            pub fn is_transparent(self) -> bool {
                match self {
                    $(
                        $( Block::$name { .. } => $transparent, )?
                    )*
                    _ => false
                }
            }

            pub fn is_cube(self) -> bool {
                match self {
                    $(
                        $( Block::$name { .. } => $cube, )?
                    )*
                    _ => false
                }
            }

            pub fn get_id(self) -> u32 {
                match self {
                    $(
                        Block::$name {
                            $(
                                $prop_name,
                            )*
                        } => $get_id,
                    )*
                }
            }

            pub fn from_id(mut id: u32) -> Block {
                match id {
                    $(
                        $from_id_pat => {
                            $( id -= $get_id_offset; )?
                            let $id_name = id;
                            Block::$name {
                                $(
                                    $from_id_pkey: $from_id_pval
                                ),*
                            }
                        },
                    )*
                }
            }

            pub fn from_name(name: &str) -> Option<Block> {
                match name {
                    $(
                        $(
                            $from_name_pat => {
                                let $name_name = name;
                                Some(Block::$name {
                                    $(
                                        $from_name_pkey: $from_name_pval
                                    ),*
                                })
                            },
                        )*
                    )*
                    _ => None,
                }
            }

            // Not all props will be part of the name
            #[allow(unused_variables)]
            pub fn get_name(self) -> &'static str {
                match self {
                    $(
                        Block::$name {
                            $(
                                $prop_name,
                            )*
                        } => $get_name,
                    )*
                }
            }

            pub fn set_properties(&mut self, props: HashMap<&str, &str>) {
                match self {
                    $(
                        Block::$name {
                            $(
                                $prop_name,
                            )*
                        } => {
                            $(
                                <$prop_type as BlockProperty>::decode($prop_name, &props, stringify!($prop_name));
                            )*
                        },
                    )*
                }
            }

            pub fn properties(&self) -> HashMap<&'static str, String> {
                let mut props = HashMap::new();
                match self {
                    $(
                        Block::$name {
                            $(
                                $prop_name,
                            )*
                        } => {
                            $(
                                <$prop_type as BlockProperty>::encode(*$prop_name, &mut props, stringify!($prop_name));
                            )*
                        },
                    )*
                }
                props
            }

            pub fn rotate(&mut self, amt: RotateAmt) {
                match self {
                    $(
                        Block::$name {
                            $(
                                $prop_name,
                            )*
                        } => {
                            $(
                                <$prop_type as BlockTransform>::rotate($prop_name, amt);
                            )*
                        },
                    )*
                }
            }

            pub fn flip(&mut self, dir: FlipDirection) {
                match self {
                    $(
                        Block::$name {
                            $(
                                $prop_name,
                            )*
                        } => {
                            $(
                                <$prop_type as BlockTransform>::flip($prop_name, dir);
                            )*
                        },
                    )*
                }
            }
        }
    }
}

blocks! {
    Air {
        props: {},
        get_id: 0,
        from_id(_id): 0 => {},
        from_names(_name): {
            "air" => {}
        },
        get_name: "air",
    },
    Stone {
        props: {},
        get_id: 1,
        from_id(_id): 1 => {},
        from_names(_name): {
            "stone" => {}
        },
        get_name: "stone",
        solid: true,
        cube: true,
    },
    Glass {
        props: {},
        get_id: 262,
        from_id(_id): 262 => {},
        from_names(_name): {
            "glass" => {}
        },
        get_name: "glass",
        transparent: true,
        cube: true,
    },
    Glowstone {
        props: {},
        get_id: 4082,
        from_id(_id): 4082 => {},
        from_names(_name): {
            "glowstone" => {}
        },
        get_name: "glowstone",
        transparent: true,
        cube: true,
    },
    RedstoneWire {
        props: {
            wire: RedstoneWire
        },
        get_id: {
            wire.east.get_id() * 432
                + wire.north.get_id() * 144
                + wire.power as u32 * 9
                + wire.south.get_id() * 3
                + wire.west.get_id()
                + 2114
        },
        from_id_offset: 2114,
        from_id(id): 2114..=3409 => {
            wire: RedstoneWire::new(
                RedstoneWireSide::from_id(id % 432 / 144),
                RedstoneWireSide::from_id(id % 9 / 3),
                RedstoneWireSide::from_id(id / 432),
                RedstoneWireSide::from_id(id % 3),
                (id % 144 / 9) as u8,
            )
        },
        from_names(_name): {
            "redstone_wire" => {
                wire: Default::default()
            }
        },
        get_name: "redstone_wire",
    },
    WallSign {
        props: {
            sign_type: u32,
            facing: BlockDirection
        },
        get_id: (sign_type << 3) + (facing.get_id() << 1) + 3803,
        from_id_offset: 3803,
        from_id(id): 3803..=3849 => {
            sign_type: id >> 3,
            facing: BlockDirection::from_id((id & 0b110) >> 1)
        },
        from_names(_name): {
            "oak_wall_sign" => {
                sign_type: 0,
                facing: Default::default()
            },
            "spruce_wall_sign" => {
                sign_type: 1,
                facing: Default::default()
            },
            "birch_wall_sign" => {
                sign_type: 2,
                facing: Default::default()
            },
            "jungle_wall_sign" => {
                sign_type: 3,
                facing: Default::default()
            },
            "acacia_wall_sign" => {
                sign_type: 4,
                facing: Default::default()
            },
            "dark_oak_wall_sign" => {
                sign_type: 5,
                facing: Default::default()
            }
        },
        get_name: match sign_type {
            0 => "oak_wall_sign",
            1 => "spruce_wall_sign",
            2 => "birch_wall_sign",
            3 => "jungle_wall_sign",
            4 => "acacia_wall_sign",
            5 => "dark_oak_wall_sign",
            _ => "invalid_wall_sign"
        },
    },
    Lever {
        props: {
            lever: Lever
        },
        get_id: {
            (lever.face.get_id() << 3)
                + (lever.facing.get_id() << 1)
                + !lever.powered as u32
                + 3850
        },
        from_id_offset: 3850,
        from_id(id): 3850..=3873 => {
            lever: Lever::new(
                LeverFace::from_id(id >> 3),
                BlockDirection::from_id((id >> 1) & 0b11),
                (id & 1) == 0
            )
        },
        from_names(_name): {
            "lever" => {
                lever: Default::default()
            }
        },
        get_name: "lever",
    },
    StoneButton {
        props: {
            button: StoneButton
        },
        get_id: {
            (button.face.get_id() << 3)
                + (button.facing.get_id() << 1)
                + !button.powered as u32
                + 3966
        },
        from_id_offset: 3966,
        from_id(id): 3966..=3989 => {
            button: StoneButton::new(ButtonFace::from_id(id >> 3), BlockDirection::from_id((id >> 1) & 0b11), (id & 1) == 0)
        },
        from_names(_name): {
            "stone_button" => {
                button: Default::default()
            }
        },
        get_name: "stone_button",
    },
    Sign {
        props: {
            sign_type: u32,
            rotation: u32
        },
        get_id: (sign_type << 5) + (rotation << 1) + 3439,
        from_id_offset: 3439,
        from_id(id): 3439..=3629 => {
            sign_type: id >> 5,
            rotation: (id & 0b11110) >> 1
        },
        from_names(_name): {
            "oak_sign" => {
                sign_type: 0,
                rotation: 0
            },
            "spruce_sign" => {
                sign_type: 1,
                rotation: 0
            },
            "birch_sign" => {
                sign_type: 2,
                rotation: 0
            },
            "jungle_sign" => {
                sign_type: 3,
                rotation: 0
            },
            "acacia_sign" => {
                sign_type: 4,
                rotation: 0
            },
            "dark_oak_sign" => {
                sign_type: 5,
                rotation: 0
            }
        },
        get_name: match sign_type {
            0 => "oak_sign",
            1 => "spruce_sign",
            2 => "birch_sign",
            3 => "jungle_sign",
            4 => "acacia_sign",
            5 => "dark_oak_sign",
            _ => "invalid_sign"
        },
    },
    RedstoneTorch {
        props: {
            lit: bool
        },
        get_id: if lit {
            3956
        } else {
            3957
        },
        from_id_offset: 3956,
        from_id(id): 3956..=3957 => {
            lit: id == 0
        },
        from_names(_name): {
            "redstone_torch" => {
                lit: true
            }
        },
        get_name: "redstone_torch",
    },
    RedstoneWallTorch {
        props: {
            lit: bool,
            facing: BlockDirection
        },
        get_id: (facing.get_id() << 1) + (!lit as u32) + 3958,
        from_id_offset: 3958,
        from_id(id): 3958..=3965 => {
            lit: (id & 1) == 0,
            facing: BlockDirection::from_id(id >> 1)
        },
        from_names(_name): {
            "redstone_wall_torch" => {
                lit: true,
                facing: Default::default()
            }
        },
        get_name: "redstone_wall_torch",
    },
    RedstoneRepeater {
        props: {
            repeater: RedstoneRepeater
        },
        get_id: {
            (repeater.delay as u32 - 1) * 16
                + repeater.facing.get_id() * 4
                + !repeater.locked as u32 * 2
                + !repeater.powered as u32
                + 4100
        },
        from_id_offset: 4100,
        from_id(id): 4100..=4163 => {
            repeater: RedstoneRepeater::new(
                (id >> 4) as u8 + 1,
                BlockDirection::from_id((id >> 2) & 3),
                ((id >> 1) & 1) == 0,
                (id & 1) == 0
            )
        },
        from_names(_name): {
            "repeater" => {
                repeater: Default::default()
            }
        },
        get_name: "repeater",
    },
    RedstoneLamp {
        props: {
            lit: bool
        },
        get_id: if lit {
            5361
        } else {
            5362
        },
        from_id_offset: 5361,
        from_id(id): 5361..=5362 => {
            lit: id == 0
        },
        from_names(_name): {
            "redstone_lamp" => {
                lit: false
            }
        },
        get_name: "redstone_lamp",
        solid: true,
        cube: true,
    },
    TripwireHook {
        props: {
            direction: BlockDirection
        },
        get_id: match direction {
            BlockDirection::North => 5474,
            BlockDirection::South => 5476,
            BlockDirection::West => 5478,
            BlockDirection::East => 5480,
        },
        from_id_offset: 5474,
        from_id(id): 5474..=5480 => {
            direction: BlockDirection::from_id(id / 2)
        },
        from_names(_name): {
            "tripwire_hook" => {
                direction: Default::default()
            }
        },
        get_name: "tripwire_hook",
    },
    RedstoneComparator {
        props: {
            comparator: RedstoneComparator
        },
        get_id: {
            comparator.facing.get_id() * 4
                + comparator.mode.get_id() * 2
                + !comparator.powered as u32
                + 6884
        },
        from_id_offset: 6884,
        from_id(id): 6884..=6899 => {
            comparator: RedstoneComparator::new(
                BlockDirection::from_id(id >> 2),
                ComparatorMode::from_id((id >> 1) & 1),
                (id & 1) == 0
            )
        },
        from_names(_name): {
            "comparator" => {
                comparator: Default::default()
            }
        },
        get_name: "comparator",
    },
    RedstoneBlock {
        props: {},
        get_id: 6932,
        from_id(_id): 6932 => {},
        from_names(_name): {
            "redstone_block" => {}
        },
        get_name: "redstone_block",
        transparent: true,
        cube: true,
    },
    Observer {
        props: {
            facing: BlockFacing
        },
        get_id: (facing.get_id() << 1) + 9510,
        from_id_offset: 9510,
        from_id(id): 9510..=9521 => {
            facing: BlockFacing::from_id(id >> 1)
        },
        from_names(_name): {
            "observer" => {
                facing: Default::default()
            }
        },
        get_name: "observer",
        solid: true,
        cube: true,
    },
    SeaPickle {
        props: {
            pickles: u8
        },
        get_id: ((pickles - 1) << 1) as u32 + 9890,
        from_id_offset: 9890,
        from_id(id): 9890..=9897 => {
            pickles: (id >> 1) as u8 + 1
        },
        from_names(_name): {
            "sea_pickle" => {
                pickles: 1
            }
        },
        get_name: "sea_pickle",
    },
    Target {
        props: {},
        get_id: 16014,
        from_id(_id): 16014 => {},
        from_names(_name): {
            "target" => {}
        },
        get_name: "target",
        solid: true,
        cube: true,
    },
    StonePressurePlate {
        props: {
            powered: bool
        },
        get_id: 3874 + !powered as u32,
        from_id_offset: 3874,
        from_id(id): 3874..=3875 => {
            powered: id == 0
        },
        from_names(_name): {
            "stone_pressure_plate" => {
                powered: false
            }
        },
        get_name: "stone_pressure_plate",
    },
    Barrel {
        props: {},
        get_id: 15042,
        from_id(_id): 15042 => {},
        from_names(_name): {
            "barrel" => {}
        },
        get_name: "barrel",
        solid: true,
        cube: true,
    },
    Hopper {
        props: {},
        get_id: 6939,
        from_id(_id): 6939 => {},
        from_names(_name): {
            "hopper" => {}
        },
        get_name: "hopper",
        transparent: true,
        cube: true,
    },
    Sandstone {
        props: {},
        get_id: 278,
        from_id(_id): 278 => {},
        from_names(_name): {
            "sandstone" => {}
        },
        get_name: "sandstone",
        solid: true,
        cube: true,
    },
    CoalBlock {
        props: {},
        get_id: 8133,
        from_id(_id): 8133 => {},
        from_names(_name): {
            "coal_block" => {}
        },
        get_name: "coal_block",
        solid: true,
        cube: true,
    },
    Furnace {
        props: {},
        get_id: 3431,
        from_id(_id): 3431 => {},
        from_names(_name): {
            "furnace" => {}
        },
        get_name: "furnace",
        solid: true,
        cube: true,
    },
    Quartz {
        props: {},
        get_id: 6944,
        from_id(_id): 6944 => {},
        from_names(_name): {
            "quartz_block" => {}
        },
        get_name: "quartz_block",
        solid: true,
        cube: true,
    },
    SmoothQuartz {
        props: {},
        get_id: 8666,
        from_id(_id): 8666 => {},
        from_names(_name): {
            "smooth_quartz" => {}
        },
        get_name: "smooth_quartz",
        solid: true,
        cube: true,
    },
    SmoothStoneSlab {
        props: {},
        get_id: 8593,
        from_id(_id): 8593 => {},
        from_names(_name): {
            "smooth_stone_slab" => {}
        },
        get_name: "smooth_stone_slab[type=top]",
        transparent: true,
        cube: true,
    },
    QuartzSlab {
        props: {},
        get_id: 8641,
        from_id(_id): 8641 => {},
        from_names(_name): {
            "quartz_slab" => {}
        },
        get_name: "quartz_slab",
        transparent: true,
        cube: true,
    },
    Cauldron {
        props: {
            level: u8
        },
        get_id: level as u32 + 5342,
        from_id_offset: 5342,
        from_id(id): 5342..=5345 => {
            level: id as u8
        },
        from_names(_name): {
            "cauldron" => {
                level: 0
            },
            "water_cauldron" => {
                level: 3
            }
        },
        get_name: match level {
            0 => "cauldron",
            _ => "water_cauldron"
        },
        transparent: true,
        cube: false,
    },
    Composter {
        props: {
            level: u8
        },
        get_id: level as u32 + 16005,
        from_id_offset: 16005,
        from_id(id): 16005..=16013 => {
            level: id as u8
        },
        from_names(_name): {
            "composter" => {
                level: 0
            }
        },
        get_name: "composter",
        transparent: true,
        // FIXME: You can place repeaters and comparators on it, but not wires?
        cube: true,
    },
    Concrete {
        props: {
            color: BlockColorVariant
        },
        get_id: color.get_id() + 9688,
        from_id_offset: 9688,
        from_id(id): 9688..=9703 => {
            color: BlockColorVariant::from_id(id)
        },
        from_names(_name): {
            "white_concrete" => { color: BlockColorVariant::White },
            "orange_concrete" => { color: BlockColorVariant::Orange },
            "magenta_concrete" => { color: BlockColorVariant::Magenta },
            "light_blue_concrete" => { color: BlockColorVariant::LightBlue },
            "yellow_concrete" => { color: BlockColorVariant::Yellow },
            "lime_concrete" => { color: BlockColorVariant::Lime },
            "pink_concrete" => { color: BlockColorVariant::Pink },
            "gray_concrete" => { color: BlockColorVariant::Gray },
            "light_gray_concrete" => { color: BlockColorVariant::LightGray },
            "cyan_concrete" => { color: BlockColorVariant::Cyan },
            "purple_concrete" => { color: BlockColorVariant::Purple },
            "blue_concrete" => { color: BlockColorVariant::Blue },
            "brown_concrete" => { color: BlockColorVariant::Brown },
            "green_concrete" => { color: BlockColorVariant::Green },
            "red_concrete" => { color: BlockColorVariant::Red },
            "black_concrete" => { color: BlockColorVariant::Black }
        },
        get_name: match color {
            BlockColorVariant::White => "white_concrete",
            BlockColorVariant::Orange => "orange_concrete",
            BlockColorVariant::Magenta => "magenta_concrete",
            BlockColorVariant::LightBlue => "light_blue_concrete",
            BlockColorVariant::Yellow => "yellow_concrete",
            BlockColorVariant::Lime => "lime_concrete",
            BlockColorVariant::Pink => "pink_concrete",
            BlockColorVariant::Gray => "gray_concrete",
            BlockColorVariant::LightGray => "light_gray_concrete",
            BlockColorVariant::Cyan => "cyan_concrete",
            BlockColorVariant::Purple => "purple_concrete",
            BlockColorVariant::Blue => "blue_concrete",
            BlockColorVariant::Brown => "brown_concrete",
            BlockColorVariant::Green => "green_concrete",
            BlockColorVariant::Red => "red_concrete",
            BlockColorVariant::Black => "black_concrete",
        },
        solid: true,
        cube: true,
    },
    StainedGlass {
        props: {
            color: BlockColorVariant
        },
        get_id: color.get_id() + 4164,
        from_id_offset: 4164,
        from_id(id): 4164..=4179 => {
            color: BlockColorVariant::from_id(id)
        },
        from_names(_name): {
            "white_stained_glass" => { color: BlockColorVariant::White },
            "orange_stained_glass" => { color: BlockColorVariant::Orange },
            "magenta_stained_glass" => { color: BlockColorVariant::Magenta },
            "light_blue_stained_glass" => { color: BlockColorVariant::LightBlue },
            "yellow_stained_glass" => { color: BlockColorVariant::Yellow },
            "lime_stained_glass" => { color: BlockColorVariant::Lime },
            "pink_stained_glass" => { color: BlockColorVariant::Pink },
            "gray_stained_glass" => { color: BlockColorVariant::Gray },
            "light_gray_stained_glass" => { color: BlockColorVariant::LightGray },
            "cyan_stained_glass" => { color: BlockColorVariant::Cyan },
            "purple_stained_glass" => { color: BlockColorVariant::Purple },
            "blue_stained_glass" => { color: BlockColorVariant::Blue },
            "brown_stained_glass" => { color: BlockColorVariant::Brown },
            "green_stained_glass" => { color: BlockColorVariant::Green },
            "red_stained_glass" => { color: BlockColorVariant::Red },
            "black_stained_glass" => { color: BlockColorVariant::Black }
        },
        get_name: match color {
            BlockColorVariant::White => "white_stained_glass",
            BlockColorVariant::Orange => "orange_stained_glass",
            BlockColorVariant::Magenta => "magenta_stained_glass",
            BlockColorVariant::LightBlue => "light_blue_stained_glass",
            BlockColorVariant::Yellow => "yellow_stained_glass",
            BlockColorVariant::Lime => "lime_stained_glass",
            BlockColorVariant::Pink => "pink_stained_glass",
            BlockColorVariant::Gray => "gray_stained_glass",
            BlockColorVariant::LightGray => "light_gray_stained_glass",
            BlockColorVariant::Cyan => "cyan_stained_glass",
            BlockColorVariant::Purple => "purple_stained_glass",
            BlockColorVariant::Blue => "blue_stained_glass",
            BlockColorVariant::Brown => "brown_stained_glass",
            BlockColorVariant::Green => "green_stained_glass",
            BlockColorVariant::Red => "red_stained_glass",
            BlockColorVariant::Black => "black_stained_glass",
        },
        transparent: true,
        cube: true,
    },
    Terracotta {
        props: {},
        get_id: 8132,
        from_id(_id): 8132 => {},
        from_names(_name): {
            "terracotta" => {}
        },
        get_name: "terracotta",
        solid: true,
        cube: true,
    },
    ColoredTerracotta {
        props: {
            color: BlockColorVariant
        },
        get_id: color.get_id() + 7065,
        from_id_offset: 7065,
        from_id(id): 7065..=7080 => {
            color: BlockColorVariant::from_id(id)
        },
        from_names(_name): {
            "white_terracotta" => { color: BlockColorVariant::White },
            "orange_terracotta" => { color: BlockColorVariant::Orange },
            "magenta_terracotta" => { color: BlockColorVariant::Magenta },
            "light_blue_terracotta" => { color: BlockColorVariant::LightBlue },
            "yellow_terracotta" => { color: BlockColorVariant::Yellow },
            "lime_terracotta" => { color: BlockColorVariant::Lime },
            "pink_terracotta" => { color: BlockColorVariant::Pink },
            "gray_terracotta" => { color: BlockColorVariant::Gray },
            "light_gray_terracotta" => { color: BlockColorVariant::LightGray },
            "cyan_terracotta" => { color: BlockColorVariant::Cyan },
            "purple_terracotta" => { color: BlockColorVariant::Purple },
            "blue_terracotta" => { color: BlockColorVariant::Blue },
            "brown_terracotta" => { color: BlockColorVariant::Brown },
            "green_terracotta" => { color: BlockColorVariant::Green },
            "red_terracotta" => { color: BlockColorVariant::Red },
            "black_terracotta" => { color: BlockColorVariant::Black }
        },
        get_name: match color {
            BlockColorVariant::White => "white_terracotta",
            BlockColorVariant::Orange => "orange_terracotta",
            BlockColorVariant::Magenta => "magenta_terracotta",
            BlockColorVariant::LightBlue => "light_blue_terracotta",
            BlockColorVariant::Yellow => "yellow_terracotta",
            BlockColorVariant::Lime => "lime_terracotta",
            BlockColorVariant::Pink => "pink_terracotta",
            BlockColorVariant::Gray => "gray_terracotta",
            BlockColorVariant::LightGray => "light_gray_terracotta",
            BlockColorVariant::Cyan => "cyan_terracotta",
            BlockColorVariant::Purple => "purple_terracotta",
            BlockColorVariant::Blue => "blue_terracotta",
            BlockColorVariant::Brown => "brown_terracotta",
            BlockColorVariant::Green => "green_terracotta",
            BlockColorVariant::Red => "red_terracotta",
            BlockColorVariant::Black => "black_terracotta",
        },
        solid: true,
        cube: true,
    },
    Wool {
        props: {
            color: BlockColorVariant
        },
        get_id: color.get_id() + 1440,
        from_id_offset: 1440,
        from_id(id): 1440..=1455 => {
            color: BlockColorVariant::from_id(id)
        },
        from_names(_name): {
            "white_wool" => { color: BlockColorVariant::White },
            "orange_wool" => { color: BlockColorVariant::Orange },
            "magenta_wool" => { color: BlockColorVariant::Magenta },
            "light_blue_wool" => { color: BlockColorVariant::LightBlue },
            "yellow_wool" => { color: BlockColorVariant::Yellow },
            "lime_wool" => { color: BlockColorVariant::Lime },
            "pink_wool" => { color: BlockColorVariant::Pink },
            "gray_wool" => { color: BlockColorVariant::Gray },
            "light_gray_wool" => { color: BlockColorVariant::LightGray },
            "cyan_wool" => { color: BlockColorVariant::Cyan },
            "purple_wool" => { color: BlockColorVariant::Purple },
            "blue_wool" => { color: BlockColorVariant::Blue },
            "brown_wool" => { color: BlockColorVariant::Brown },
            "green_wool" => { color: BlockColorVariant::Green },
            "red_wool" => { color: BlockColorVariant::Red },
            "black_wool" => { color: BlockColorVariant::Black }
        },
        get_name: match color {
            BlockColorVariant::White => "white_wool",
            BlockColorVariant::Orange => "orange_wool",
            BlockColorVariant::Magenta => "magenta_wool",
            BlockColorVariant::LightBlue => "light_blue_wool",
            BlockColorVariant::Yellow => "yellow_wool",
            BlockColorVariant::Lime => "lime_wool",
            BlockColorVariant::Pink => "pink_wool",
            BlockColorVariant::Gray => "gray_wool",
            BlockColorVariant::LightGray => "light_gray_wool",
            BlockColorVariant::Cyan => "cyan_wool",
            BlockColorVariant::Purple => "purple_wool",
            BlockColorVariant::Blue => "blue_wool",
            BlockColorVariant::Brown => "brown_wool",
            BlockColorVariant::Green => "green_wool",
            BlockColorVariant::Red => "red_wool",
            BlockColorVariant::Black => "black_wool",
        },
        solid: true,
        cube: true,
    },
    IronTrapdoor {
        props: {
            facing: BlockFacing,
            half: TrapdoorHalf,
            powered: bool
        },
        get_id: {
            facing.get_id() * 16
                + half.get_id() * 8
                + !powered as u32 * 6
                + 7788
        },
        from_id_offset: 7788,
        from_id(id): 7788..=7850 => {
            facing: BlockFacing::from_id(id >> 4),
            half: TrapdoorHalf::from_id((id >> 3) & 1),
            powered: ((id >> 1) & 1) == 0
        },
        from_names(_name): {
            "iron_trapdoor" => {
                facing: Default::default(),
                half: TrapdoorHalf::Bottom,
                powered: false
            }
        },
        get_name: "iron_trapdoor",
    },
    Unknown {
        props: {
            id: u32
        },
        get_id: id,
        from_id(id): _ => { id: id },
        from_names(name): {},
        get_name: "unknown",
        solid: true,
        cube: true,
    }
}
