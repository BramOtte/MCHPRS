use super::{PacketEncoder, PacketEncoderExt, PalettedContainer, PlayerProperty, SlotData};
use crate::generated::status::clientbound as status;
use crate::generated::configuration::clientbound as config;
use crate::generated::login::clientbound as login;
use crate::generated::play::clientbound as play;
use crate::nbt_util::{NBTCompound, NBTMap};
use bitvec::bits;
use bitvec::prelude::Lsb0;
use mchprs_proc_macros::protocol_id;
use mchprs_text::TextComponent;
use serde::Serialize;
use std::collections::HashMap;

pub trait ClientBoundPacket {
    fn encode(&self) -> PacketEncoder;
}

fn encode_plugin_message(packet_id: u32, channel: &str, data: &[u8]) -> PacketEncoder {
    let mut buf = Vec::new();
    buf.write_string(32767, channel);
    buf.write_bytes(data);
    PacketEncoder::new(buf, packet_id)
}

// Server List Ping Packets

pub struct CResponse {
    pub json_response: String,
}

impl ClientBoundPacket for CResponse {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_string(32767, &self.json_response);
        PacketEncoder::new(buf, status::STATUS_RESPONSE)
    }
}

// Login Packets

pub struct CDisconnectLogin {
    pub reason: String,
}

impl ClientBoundPacket for CDisconnectLogin {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_string(32767, &self.reason);
        PacketEncoder::new(buf, login::LOGIN_DISCONNECT)
    }
}

pub struct CPong {
    pub payload: i64,
}

impl ClientBoundPacket for CPong {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_long(self.payload);
        PacketEncoder::new(buf, login::HELLO)
    }
}

pub struct CLoginSuccess {
    pub uuid: u128,
    pub username: String,
    pub properties: Vec<PlayerProperty>,
}

impl ClientBoundPacket for CLoginSuccess {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_uuid(self.uuid);
        buf.write_string(16, &self.username);
        buf.write_varint(self.properties.len() as i32);
        for prop in &self.properties {
            buf.write_player_property(prop);
        }
        PacketEncoder::new(buf, login::LOGIN_FINISHED)
    }
}

pub struct CSetCompression {
    pub threshold: i32,
}

impl ClientBoundPacket for CSetCompression {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.threshold);
        PacketEncoder::new(buf, login::LOGIN_COMPRESSION)
    }
}

pub struct CLoginPluginRequest {
    pub message_id: i32,
    pub channel: String,
    pub data: Vec<u8>,
}

impl ClientBoundPacket for CLoginPluginRequest {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.message_id);
        buf.write_identifier(&self.channel);
        buf.write_bytes(&self.data);
        PacketEncoder::new(buf, login::CUSTOM_QUERY)
    }
}

// Configuration Packets

pub struct CConfigurationPluginMessage {
    pub channel: String,
    pub data: Vec<u8>,
}

impl ClientBoundPacket for CConfigurationPluginMessage {
    fn encode(&self) -> PacketEncoder {
        encode_plugin_message(config::CUSTOM_PAYLOAD, &self.channel, &self.data)
    }
}

pub struct CFinishConfiguration;

impl ClientBoundPacket for CFinishConfiguration {
    fn encode(&self) -> PacketEncoder {
        PacketEncoder::new(Vec::new(), config::FINISH_CONFIGURATION)
    }
}

#[derive(Serialize, Clone)]
pub struct CRegistryDimensionType {
    pub fixed_time: Option<i64>,
    pub has_skylight: bool,
    pub has_ceiling: bool,
    pub ultrawarm: bool,
    pub natural: bool,
    pub coordinate_scale: f32,
    pub bed_works: bool,
    pub respawn_anchor_works: bool,
    pub min_y: i32,
    pub height: i32,
    pub logical_height: i32,
    pub infiniburn: String,
    pub effects: String,
    pub ambient_light: f32,
    pub piglin_safe: bool,
    pub has_raids: bool,
    pub monster_spawn_light_level: i8,
    pub monster_spawn_block_light_limit: i8,
}

#[derive(Serialize, Clone)]
pub struct CRegistryBiomeEffects {
    pub fog_color: i32,
    pub water_color: i32,
    pub water_fog_color: i32,
    pub sky_color: i32,
}

#[derive(Serialize, Clone)]
pub struct CRegistryBiome {
    // serializied as byte tag
    pub has_precipitation: bool,
    pub temperature: f32,
    pub downfall: f32,
    pub effects: CRegistryBiomeEffects,
}

#[derive(Serialize, Clone, Default)]
pub struct CRegistryDamageType {
    message_id: String,
    scaling: String,
    exhaustion: f32,
}

pub struct CRegistryDataCodec {
    /// The `minecraft:dimension_type registry`. It defines the types of dimension that can be
    /// attributed to a world, along with all their characteristics.
    pub dimension_types: HashMap<String, CRegistryDimensionType>,
    /// The `minecraft:worldgen/biome` registry. It defines several aesthetic characteristics of
    /// the biomes present in the game.
    pub biomes: HashMap<String, CRegistryBiome>,
}

#[derive(Serialize)]
struct CRegistryDataCodecInner {
    #[serde(rename = "minecraft:dimension_type")]
    pub dimension_types: NBTMap<CRegistryDimensionType>,
    #[serde(rename = "minecraft:worldgen/biome")]
    pub biomes: NBTMap<CRegistryBiome>,
    #[serde(rename = "minecraft:damage_type")]
    pub damage_types: NBTMap<CRegistryDamageType>,
}

impl CRegistryDataCodec {
    fn encode(&self, buf: &mut Vec<u8>) {
        let mut dimension_map: NBTMap<CRegistryDimensionType> =
            NBTMap::new("minecraft:dimension_type".to_owned());
        for (name, element) in &self.dimension_types {
            dimension_map.push_element(name.clone(), element.clone());
        }
        let mut biome_map = NBTMap::new("minecraft:worldgen/biome".to_owned());
        for (name, element) in &self.biomes {
            biome_map.push_element(name.clone(), element.clone());
        }

        // The game will throw if it doesn't have these. See MC-267103.
        let required_types = [
            "in_fire",
            "lightning_bolt",
            "on_fire",
            "lava",
            "hot_floor",
            "in_wall",
            "cramming",
            "drown",
            "starve",
            "cactus",
            "fall",
            "fly_into_wall",
            "out_of_world",
            "fell_out_of_world",
            "generic",
            "magic",
            "wither",
            "dragon_breath",
            "dry_out",
            "sweet_berry_bush",
            "freeze",
            "stalagmite",
            "outside_border",
            "generic_kill",
            "player_attack",
        ];
        let mut damage_map = NBTMap::new("minecraft:damage_type".to_owned());
        for ty in required_types {
            damage_map.push_element(
                format!("minecraft:{ty}"),
                CRegistryDamageType {
                    message_id: "generic".into(),
                    scaling: "always".into(),
                    ..Default::default()
                },
            );
        }

        let codec = CRegistryDataCodecInner {
            dimension_types: dimension_map,
            biomes: biome_map,
            damage_types: damage_map,
        };
        buf.write_nbt(&codec);
    }
}

pub struct CRegistryData {
    pub registry_codec: CRegistryDataCodec,
}

impl ClientBoundPacket for CRegistryData {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        self.registry_codec.encode(&mut buf);
        PacketEncoder::new(buf, config::REGISTRY_DATA)
    }
}

// Play Packets

pub struct CSpawnEntity {
    pub entity_id: i32,
    pub entity_uuid: u128,
    pub entity_type: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub pitch: f32,
    pub yaw: f32,
    pub head_yaw: f32,
    pub data: i32,
    pub velocity_x: i16,
    pub velocity_y: i16,
    pub velocity_z: i16,
}

impl ClientBoundPacket for CSpawnEntity {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.entity_id);
        buf.write_uuid(self.entity_uuid);
        buf.write_varint(self.entity_type);
        buf.write_double(self.x);
        buf.write_double(self.y);
        buf.write_double(self.z);
        buf.write_byte(((self.pitch / 360f32 * 256f32) as i32 % 256) as i8);
        buf.write_byte(((self.yaw / 360f32 * 256f32) as i32 % 256) as i8);
        buf.write_byte(((self.head_yaw / 360f32 * 256f32) as i32 % 256) as i8);
        buf.write_varint(self.data);
        buf.write_short(self.velocity_x);
        buf.write_short(self.velocity_y);
        buf.write_short(self.velocity_z);
        PacketEncoder::new(buf, play::ADD_ENTITY)
    }
}

pub struct CEntityAnimation {
    pub entity_id: i32,
    pub animation: u8,
}

impl ClientBoundPacket for CEntityAnimation {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.entity_id);
        buf.write_unsigned_byte(self.animation);
        PacketEncoder::new(buf, play::ANIMATE)
    }
}

pub struct CAcknowledgeBlockChange {
    pub sequence_id: i32,
}

impl ClientBoundPacket for CAcknowledgeBlockChange {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.sequence_id);
        PacketEncoder::new(buf, play::BLOCK_CHANGED_ACK)
    }
}

pub struct CBlockEntityData {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub ty: i32,
    pub nbt: NBTCompound,
}

impl ClientBoundPacket for CBlockEntityData {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_position(self.x, self.y, self.z);
        buf.write_varint(self.ty);
        buf.write_nbt(&self.nbt);
        PacketEncoder::new(buf, play::BLOCK_ENTITY_DATA)
    }
}

pub struct CBlockUpdate {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block_id: i32,
}

impl ClientBoundPacket for CBlockUpdate {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_position(self.x, self.y, self.z);
        buf.write_varint(self.block_id);
        PacketEncoder::new(buf, play::BLOCK_UPDATE)
    }
}

pub struct CCommandSuggestionsResponseMatch {
    pub match_: String,
    pub tooltip: Option<TextComponent>,
}

pub struct CCommandSuggestionsResponse {
    pub id: i32,
    pub start: i32,
    pub length: i32,
    pub matches: Vec<CCommandSuggestionsResponseMatch>,
}

impl ClientBoundPacket for CCommandSuggestionsResponse {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.id);
        buf.write_varint(self.start);
        buf.write_varint(self.length);
        buf.write_varint(self.matches.len() as i32);
        for m in &self.matches {
            buf.write_string(32767, &m.match_);
            buf.write_bool(m.tooltip.is_some());
            if let Some(tooltip) = &m.tooltip {
                buf.write_text_component(tooltip);
            }
        }

        PacketEncoder::new(buf, play::COMMAND_SUGGESTIONS)
    }
}

#[derive(Debug)]
pub enum CDeclareCommandsNodeParser {
    Entity(i8),
    Vec2,
    Vec3,
    Integer(i32, i32),
    Float(f32, f32),
    BlockPos,
    BlockState,
    String(i32),
}

impl CDeclareCommandsNodeParser {
    fn write(&self, buf: &mut Vec<u8>) {
        use CDeclareCommandsNodeParser::*;
        macro_rules! arg_type {
            ($name:literal) => {
                protocol_id!("minecraft:command_argument_type", $name)
            };
        }
        match self {
            Entity(flags) => {
                buf.write_varint(arg_type!("minecraft:entity"));
                buf.write_byte(*flags);
            }
            Vec2 => buf.write_varint(arg_type!("minecraft:vec2")),
            Vec3 => buf.write_varint(arg_type!("minecraft:vec3")),
            BlockPos => buf.write_varint(arg_type!("minecraft:block_pos")),
            BlockState => buf.write_varint(arg_type!("minecraft:block_state")),
            Integer(min, max) => {
                buf.write_varint(arg_type!("brigadier:integer"));
                buf.write_byte(3); // Supply min and max value
                buf.write_int(*min);
                buf.write_int(*max);
            }
            Float(min, max) => {
                buf.write_varint(arg_type!("brigadier:float"));
                buf.write_byte(3);
                buf.write_float(*min);
                buf.write_float(*max);
            }
            String(ty) => {
                buf.write_varint(arg_type!("brigadier:string"));
                buf.write_varint(*ty);
            }
        }
    }
}

#[derive(Debug)]
pub struct CCommandsNode {
    pub flags: i8,
    pub children: Vec<i32>,
    pub redirect_node: Option<i32>,
    pub name: Option<&'static str>,
    pub parser: Option<CDeclareCommandsNodeParser>,
    pub suggestions_type: Option<&'static str>,
}

pub struct CCommands {
    pub nodes: Vec<CCommandsNode>,
    pub root_index: i32,
}

impl ClientBoundPacket for CCommands {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.nodes.len() as i32);
        for node in &self.nodes {
            buf.write_byte(node.flags);
            buf.write_varint(node.children.len() as i32);
            for child in &node.children {
                buf.write_varint(*child);
            }
            if let Some(redirect_node) = node.redirect_node {
                buf.write_varint(redirect_node);
            }
            if let Some(name) = node.name {
                buf.write_string(32767, name);
            }
            if let Some(parser) = &node.parser {
                parser.write(&mut buf);
            }
            if let Some(suggesstions_type) = node.suggestions_type {
                buf.write_string(32767, suggesstions_type);
            }
        }
        buf.write_varint(self.root_index);
        PacketEncoder::new(buf, play::COMMANDS)
    }
}

pub struct CSetContainerContent {
    pub window_id: u8,
    pub state_id: i32,
    pub slot_data: Vec<Option<SlotData>>,
    pub carried_item: Option<SlotData>,
}

impl ClientBoundPacket for CSetContainerContent {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_unsigned_byte(self.window_id);
        buf.write_varint(self.state_id);
        buf.write_varint(self.slot_data.len() as i32);
        for slot_data in &self.slot_data {
            buf.write_slot_data(slot_data);
        }
        buf.write_slot_data(&self.carried_item);
        PacketEncoder::new(buf, play::CONTAINER_SET_CONTENT)
    }
}

pub struct CSetContainerSlot {
    pub window_id: u8,
    pub state_id: i32,
    pub slot: i16,
    pub slot_data: Option<SlotData>,
}

impl ClientBoundPacket for CSetContainerSlot {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_unsigned_byte(self.window_id);
        buf.write_varint(self.state_id);
        buf.write_short(self.slot);
        buf.write_slot_data(&self.slot_data);
        PacketEncoder::new(buf, play::CONTAINER_SET_SLOT)
    }
}

pub struct CPlayPluginMessage {
    pub channel: String,
    pub data: Vec<u8>,
}

impl ClientBoundPacket for CPlayPluginMessage {
    fn encode(&self) -> PacketEncoder {
        encode_plugin_message(0x18, &self.channel, &self.data)
    }
}

pub struct CDisconnect {
    pub reason: TextComponent,
}

impl ClientBoundPacket for CDisconnect {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_text_component(&self.reason);
        PacketEncoder::new(buf, play::DISCONNECT)
    }
}

#[derive(Debug)]
pub struct CUnloadChunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
}

impl ClientBoundPacket for CUnloadChunk {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_int(self.chunk_z);
        buf.write_int(self.chunk_x);
        PacketEncoder::new(buf, play::FORGET_LEVEL_CHUNK)
    }
}

pub enum CGameEventType {
    ChangeGamemode,
    /// Start waiting for level chunks
    WaitForChunks,
}

pub struct CGameEvent {
    pub reason: CGameEventType,
    pub value: f32,
}

impl ClientBoundPacket for CGameEvent {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        match self.reason {
            CGameEventType::ChangeGamemode => buf.write_unsigned_byte(3),
            CGameEventType::WaitForChunks => buf.write_unsigned_byte(13),
        }
        buf.write_float(self.value);
        PacketEncoder::new(buf, play::GAME_EVENT)
    }
}

pub struct CKeepAlive {
    pub id: i64,
}

impl ClientBoundPacket for CKeepAlive {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_long(self.id);
        PacketEncoder::new(buf, play::KEEP_ALIVE)
    }
}

pub struct CChunkDataSection {
    pub block_count: i16,
    pub block_states: PalettedContainer,
    pub biomes: PalettedContainer,
}

pub struct CChunkDataBlockEntity {
    pub x: i8,
    pub z: i8,
    pub y: i16,
    pub ty: i32,
    pub data: NBTCompound,
}

/// Chunk Data and Update Light
pub struct CChunkData {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub heightmaps: NBTCompound,
    pub chunk_sections: Vec<CChunkDataSection>,
    pub block_entities: Vec<CChunkDataBlockEntity>,
}

impl ClientBoundPacket for CChunkData {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_int(self.chunk_x);
        buf.write_int(self.chunk_z);
        buf.write_nbt(&self.heightmaps);
        let mut data = Vec::new();
        for chunk_section in &self.chunk_sections {
            data.write_short(chunk_section.block_count);
            let containers = [&chunk_section.block_states, &chunk_section.biomes];
            for container in containers {
                data.write_unsigned_byte(container.bits_per_entry);

                // Palette
                if container.bits_per_entry == 0 {
                    // Single valued palette
                    let palette = container
                        .palette
                        .as_ref()
                        .expect("container with 0 bits per entry should have palette");
                    let item = *palette.first().expect(
                        "container with 0 bits per entry should have palette with one entry",
                    );
                    data.write_varint(item);
                } else if let Some(palette) = &container.palette {
                    // Indirect palette
                    data.write_varint(palette.len() as i32);
                    for palette_entry in palette {
                        data.write_varint(*palette_entry);
                    }
                }

                // Data Array
                data.write_varint(container.data_array.len() as i32);
                for long in &container.data_array {
                    data.write_long(*long as i64);
                }
            }
        }
        buf.write_varint(data.len() as i32);
        buf.write_bytes(&data);
        // Number of block entities
        buf.write_varint(self.block_entities.len() as i32);
        for block_entity in &self.block_entities {
            buf.write_byte((block_entity.x << 4) | block_entity.z);
            buf.write_short(block_entity.y);
            buf.write_varint(block_entity.ty);
            buf.write_nbt(&block_entity.data);
        }

        // We don't do lighting because we have max ambient light
        // These will all be zeros

        // Sky Light Mask
        buf.write_varint(0);
        // Block Light Mask
        buf.write_varint(0);

        let bits = bits![u64, Lsb0; 1].repeat(self.chunk_sections.len() + 2);
        let longs = bits.as_raw_slice();
        // Empty Sky Light Mask
        buf.write_varint(longs.len() as i32);
        longs.iter().for_each(|&x| buf.write_long(x as i64));
        // Empty Block Light Mask
        buf.write_varint(longs.len() as i32);
        longs.iter().for_each(|&x| buf.write_long(x as i64));

        // Sky Light array count
        buf.write_varint(0);
        // Block Light array count
        buf.write_varint(0);

        PacketEncoder::new(buf, play::LEVEL_CHUNK_WITH_LIGHT)
    }
}

pub struct CWorldEvent {
    pub event: i32,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub data: i32,
    pub disable_relative_volume: bool,
}

impl ClientBoundPacket for CWorldEvent {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_int(self.event);
        buf.write_position(self.x, self.y, self.z);
        buf.write_int(self.data);
        buf.write_bool(self.disable_relative_volume);
        PacketEncoder::new(buf, play::LEVEL_EVENT)
    }
}

pub struct CLoginDeathLocation {
    dimension_name: String,
    x: i32,
    y: i32,
    z: i32,
}

pub struct CLogin {
    pub entity_id: i32,
    pub is_hardcore: bool,
    pub dimension_names: Vec<String>,
    pub max_players: i32,
    pub view_distance: i32,
    pub simulation_distance: i32,
    pub reduced_debug_info: bool,
    pub enable_respawn_screen: bool,
    pub do_limited_crafting: bool,
    pub dimension_type: String,
    pub dimension_name: String,
    pub hashed_seed: u64,
    pub gamemode: u8,
    pub previous_gamemode: i8,
    pub is_debug: bool,
    pub is_flat: bool,
    pub death_location: Option<CLoginDeathLocation>,
    pub portal_cooldown: i32,
}

impl ClientBoundPacket for CLogin {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_int(self.entity_id);
        buf.write_bool(self.is_hardcore);
        buf.write_varint(self.dimension_names.len() as i32);
        for name in &self.dimension_names {
            buf.write_identifier(name);
        }
        buf.write_varint(self.max_players);
        buf.write_varint(self.view_distance);
        buf.write_varint(self.simulation_distance);
        buf.write_bool(self.reduced_debug_info);
        buf.write_bool(self.enable_respawn_screen);
        buf.write_bool(self.do_limited_crafting);
        buf.write_identifier(&self.dimension_type);
        buf.write_identifier(&self.dimension_name);
        buf.write_long(self.hashed_seed as i64);
        buf.write_unsigned_byte(self.gamemode);
        buf.write_byte(self.previous_gamemode);
        buf.write_bool(self.is_debug);
        buf.write_bool(self.is_flat);
        buf.write_bool(self.death_location.is_some());
        if let Some(death_location) = &self.death_location {
            buf.write_identifier(&death_location.dimension_name);
            buf.write_position(death_location.x, death_location.y, death_location.z);
        }
        buf.write_varint(self.portal_cooldown);
        PacketEncoder::new(buf, play::LOGIN)
    }
}

pub struct COpenSignEditor {
    pub pos_x: i32,
    pub pos_y: i32,
    pub pos_z: i32,
    pub is_front_text: bool,
}

impl ClientBoundPacket for COpenSignEditor {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_position(self.pos_x, self.pos_y, self.pos_z);
        buf.write_bool(self.is_front_text);
        PacketEncoder::new(buf, play::OPEN_SIGN_EDITOR)
    }
}

pub struct CUpdateEntityPosition {
    pub entity_id: i32,
    pub delta_x: i16,
    pub delta_y: i16,
    pub delta_z: i16,
    pub on_ground: bool,
}

impl ClientBoundPacket for CUpdateEntityPosition {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.entity_id);
        buf.write_short(self.delta_x);
        buf.write_short(self.delta_y);
        buf.write_short(self.delta_z);
        buf.write_bool(self.on_ground);
        PacketEncoder::new(buf, play::MOVE_ENTITY_POS)
    }
}

pub struct CUpdateEntityPositionAndRotation {
    pub entity_id: i32,
    pub delta_x: i16,
    pub delta_y: i16,
    pub delta_z: i16,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
}

impl ClientBoundPacket for CUpdateEntityPositionAndRotation {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.entity_id);
        buf.write_short(self.delta_x);
        buf.write_short(self.delta_y);
        buf.write_short(self.delta_z);
        buf.write_byte(((self.yaw / 360f32 * 256f32) as i32 % 256) as i8);
        buf.write_byte(((self.pitch / 360f32 * 256f32) as i32 % 256) as i8);
        buf.write_bool(self.on_ground);
        PacketEncoder::new(buf, play::MOVE_ENTITY_POS_ROT)
    }
}

pub struct CEntityRotation {
    pub entity_id: i32,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
}

impl ClientBoundPacket for CEntityRotation {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.entity_id);
        buf.write_byte(((self.yaw / 360f32 * 256f32) as i32 % 256) as i8);
        buf.write_byte(((self.pitch / 360f32 * 256f32) as i32 % 256) as i8);
        buf.write_bool(self.on_ground);
        PacketEncoder::new(buf, play::MOVE_ENTITY_ROT)
    }
}

pub struct COpenScreen {
    pub window_id: i32,
    pub window_type: i32,
    pub window_title: TextComponent,
}

impl ClientBoundPacket for COpenScreen {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.window_id);
        buf.write_varint(self.window_type);
        buf.write_text_component(&self.window_title);
        PacketEncoder::new(buf, play::OPEN_SCREEN)
    }
}

pub struct CPlayerAbilities {
    pub flags: u8,
    pub fly_speed: f32,
    pub fov_modifier: f32,
}

impl ClientBoundPacket for CPlayerAbilities {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_unsigned_byte(self.flags);
        buf.write_float(self.fly_speed);
        buf.write_float(self.fov_modifier);
        PacketEncoder::new(buf, play::PLAYER_ABILITIES)
    }
}

pub struct CPlayerInfoRemove {
    pub players: Vec<u128>,
}

impl ClientBoundPacket for CPlayerInfoRemove {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.players.len() as i32);
        for &uuid in &self.players {
            buf.write_uuid(uuid);
        }
        PacketEncoder::new(buf, play::PLAYER_INFO_REMOVE)
    }
}

pub struct CPlayerInfoAddPlayer {
    pub name: String,
    pub properties: Vec<PlayerProperty>,
}

#[derive(Default)]
pub struct CPlayerInfoActions {
    pub add_player: Option<CPlayerInfoAddPlayer>,
    pub update_gamemode: Option<i32>,
    pub update_listed: Option<bool>,
    pub update_latency: Option<i32>,
    pub update_display_name: Option<Option<TextComponent>>,
}

impl CPlayerInfoActions {
    fn get_mask(&self) -> u8 {
        let mut mask = 0;

        if self.add_player.is_some() {
            mask |= 0x01;
        }
        if self.update_gamemode.is_some() {
            mask |= 0x04;
        }
        if self.update_listed.is_some() {
            mask |= 0x08;
        }
        if self.update_latency.is_some() {
            mask |= 0x10;
        }
        if self.update_display_name.is_some() {
            mask |= 0x20;
        }

        mask
    }

    fn encode(&self, buf: &mut Vec<u8>) {
        if let Some(add_player) = &self.add_player {
            buf.write_string(16, &add_player.name);
            buf.write_varint(add_player.properties.len() as i32);
            for prop in &add_player.properties {
                buf.write_player_property(prop);
            }
        }
        if let Some(gamemode) = self.update_gamemode {
            buf.write_varint(gamemode);
        }
        if let Some(listed) = self.update_listed {
            buf.write_bool(listed);
        }
        if let Some(latency) = self.update_latency {
            buf.write_varint(latency);
        }
        if let Some(display_name) = &self.update_display_name {
            buf.write_bool(display_name.is_some());
            if let Some(display_name) = display_name {
                buf.write_text_component(display_name);
            }
        }
    }
}

pub struct CPlayerInfoUpdatePlayer {
    pub uuid: u128,
    pub actions: CPlayerInfoActions,
}

pub struct CPlayerInfoUpdate {
    // All player actions must have the same fields filled out
    pub players: Vec<CPlayerInfoUpdatePlayer>,
}

impl ClientBoundPacket for CPlayerInfoUpdate {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        let mask = self
            .players
            .first()
            .map(|player| player.actions.get_mask())
            .unwrap_or(0);
        buf.write_unsigned_byte(mask);
        buf.write_varint(self.players.len() as i32);
        for player in &self.players {
            buf.write_uuid(player.uuid);
            player.actions.encode(&mut buf);
        }
        PacketEncoder::new(buf, play::PLAYER_INFO_UPDATE)
    }
}

pub struct CSynchronizePlayerPosition {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub flags: u8,
    pub teleport_id: i32,
}

impl ClientBoundPacket for CSynchronizePlayerPosition {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_double(self.x);
        buf.write_double(self.y);
        buf.write_double(self.z);
        buf.write_float(self.yaw);
        buf.write_float(self.pitch);
        buf.write_unsigned_byte(self.flags);
        buf.write_varint(self.teleport_id);
        PacketEncoder::new(buf, play::PLAYER_POSITION)
    }
}

pub struct CRemoveEntities {
    pub entity_ids: Vec<i32>,
}

impl ClientBoundPacket for CRemoveEntities {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.entity_ids.len() as i32);
        for &entity_id in &self.entity_ids {
            buf.write_varint(entity_id);
        }
        PacketEncoder::new(buf, play::REMOVE_ENTITIES)
    }
}

pub struct CResetScore {
    pub entity_name: String,
    pub objective_name: Option<String>,
}

impl ClientBoundPacket for CResetScore {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_string(32767, &self.entity_name);
        buf.write_bool(self.objective_name.is_some());
        if let Some(objective_name) = &self.objective_name {
            buf.write_string(32767, objective_name);
        }
        PacketEncoder::new(buf, play::RESET_SCORE)
    }
}

pub struct CSetHeadRotation {
    pub entity_id: i32,
    pub head_yaw: f32,
}

impl ClientBoundPacket for CSetHeadRotation {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.entity_id);
        buf.write_byte(((self.head_yaw / 360f32 * 256f32) as i32 % 256) as i8);
        PacketEncoder::new(buf, play::ROTATE_HEAD)
    }
}

#[derive(Debug, Clone)]
pub struct CUpdateSectionBlocksRecord {
    pub x: u8,
    pub y: u8,
    pub z: u8,
    pub block_id: u32,
}

#[derive(Debug, Clone)]
pub struct CUpdateSectionBlocks {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub chunk_y: u32,
    pub records: Vec<CUpdateSectionBlocksRecord>,
}

impl ClientBoundPacket for CUpdateSectionBlocks {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::with_capacity(self.records.len() * 8 + 12);
        let pos = ((self.chunk_x as i64 & 0x3FFFFF) << 42)
            | ((self.chunk_z as i64 & 0x3FFFFF) << 20)
            | (self.chunk_y as i64 & 0xFFFFF);
        buf.write_long(pos);
        buf.write_varint(self.records.len() as i32); // Length of record array
        for record in &self.records {
            let long = ((record.block_id as u64) << 12)
                | ((record.x as u64) << 8)
                | ((record.z as u64) << 4)
                | (record.y as u64);
            buf.write_varlong(long as i64);
        }

        PacketEncoder::new(buf, play::SECTION_BLOCKS_UPDATE)
    }
}

pub struct CSetHeldItem {
    pub slot: i8,
}

impl ClientBoundPacket for CSetHeldItem {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_byte(self.slot);
        PacketEncoder::new(buf, play::SET_HELD_SLOT)
    }
}

pub struct CSetCenterChunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
}

impl ClientBoundPacket for CSetCenterChunk {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.chunk_x);
        buf.write_varint(self.chunk_z);
        PacketEncoder::new(buf, play::SET_CHUNK_CACHE_CENTER)
    }
}

pub struct CDisplayObjective {
    pub position: u8,
    pub score_name: String,
}

impl ClientBoundPacket for CDisplayObjective {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_byte(self.position as i8);
        buf.write_string(32767, &self.score_name);
        PacketEncoder::new(buf, play::SET_DISPLAY_OBJECTIVE)
    }
}

pub struct CSetEntityMetadataEntry {
    pub index: u8,
    pub metadata_type: i32,
    pub value: Vec<u8>,
}

pub struct CSetEntityMetadata {
    pub entity_id: i32,
    pub metadata: Vec<CSetEntityMetadataEntry>,
}

impl ClientBoundPacket for CSetEntityMetadata {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.entity_id);
        for entry in &self.metadata {
            buf.write_unsigned_byte(entry.index);
            buf.write_varint(entry.metadata_type);
            buf.write_bytes(&entry.value);
        }
        buf.write_byte(-1); // 0xFF
        PacketEncoder::new(buf, play::SET_ENTITY_DATA)
    }
}

pub struct CSetEquipmentEquipment {
    pub slot: i8,
    pub item: Option<SlotData>,
}

pub struct CSetEquipment {
    pub entity_id: i32,
    pub equipment: Vec<CSetEquipmentEquipment>,
}

impl ClientBoundPacket for CSetEquipment {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.entity_id);
        for slot in &self.equipment {
            buf.write_byte(slot.slot);
            buf.write_slot_data(&slot.item);
        }

        PacketEncoder::new(buf, play::SET_EQUIPMENT)
    }
}

pub enum ObjectiveNumberFormat {
    Blank,
    Styled { styling: NBTCompound },
    Fixed { content: TextComponent },
}

impl ObjectiveNumberFormat {
    fn write_to_buf(&self, buf: &mut Vec<u8>) {
        match self {
            ObjectiveNumberFormat::Blank => {
                buf.write_varint(0);
            }
            ObjectiveNumberFormat::Styled { styling } => {
                buf.write_varint(1);
                buf.write_nbt(styling);
            }
            ObjectiveNumberFormat::Fixed { content } => {
                buf.write_varint(2);
                buf.write_text_component(content);
            }
        };
    }
}

pub struct CUpdateScore {
    pub entity_name: String,
    pub objective_name: String,
    pub value: i32,
    pub display_name: Option<TextComponent>,
    pub number_format: Option<ObjectiveNumberFormat>,
}

impl ClientBoundPacket for CUpdateScore {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_string(32767, &self.entity_name);
        buf.write_string(32767, &self.objective_name);
        buf.write_varint(self.value);
        buf.write_bool(self.display_name.is_some());
        if let Some(display_name) = &self.display_name {
            buf.write_text_component(display_name);
        }
        buf.write_bool(self.number_format.is_some());
        if let Some(number_format) = &self.number_format {
            number_format.write_to_buf(&mut buf);
        }
        PacketEncoder::new(buf, play::SET_SCORE)
    }
}

pub struct CUpdateObjectives {
    pub objective_name: String,
    pub mode: u8,
    pub objective_value: TextComponent,
    pub ty: u32,
    pub number_format: Option<ObjectiveNumberFormat>,
}

impl ClientBoundPacket for CUpdateObjectives {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_string(16, &self.objective_name);
        buf.write_byte(self.mode as i8);
        if self.mode == 0 || self.mode == 2 {
            buf.write_text_component(&self.objective_value);
            buf.write_varint(self.ty as i32);
            if let Some(number_format) = &self.number_format {
                number_format.write_to_buf(&mut buf);
            }
        }
        PacketEncoder::new(buf, play::SET_OBJECTIVE)
    }
}

pub struct UpdateTime {
    pub world_age: i64,
    pub time_of_day: i64,
}

impl ClientBoundPacket for UpdateTime {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_long(self.world_age);
        buf.write_long(self.time_of_day);
        PacketEncoder::new(buf, play::SET_TIME)
    }
}

pub struct CSoundEffect {
    pub sound_id: i32,
    pub sound_name: Option<String>,
    pub has_fixed_range: Option<bool>,
    pub range: Option<bool>,
    pub sound_category: i32,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub volume: f32,
    pub pitch: f32,
    pub seed: i64,
}

impl ClientBoundPacket for CSoundEffect {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.sound_id);
        buf.write_varint(self.sound_category);
        buf.write_int(self.x);
        buf.write_int(self.y);
        buf.write_int(self.z);
        buf.write_float(self.volume);
        buf.write_float(self.pitch);
        buf.write_long(self.seed);
        PacketEncoder::new(buf, play::SOUND)
    }
}

pub struct CTeleportEntity {
    pub entity_id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
}

impl ClientBoundPacket for CTeleportEntity {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_varint(self.entity_id);
        buf.write_double(self.x);
        buf.write_double(self.y);
        buf.write_double(self.z);
        buf.write_byte(((self.yaw / 360f32 * 256f32) as i32 % 256) as i8);
        buf.write_byte(((self.pitch / 360f32 * 256f32) as i32 % 256) as i8);
        buf.write_bool(self.on_ground);
        PacketEncoder::new(buf, play::TELEPORT_ENTITY)
    }
}

pub struct CSystemChatMessage {
    pub content: TextComponent,
    pub overlay: bool,
}

impl ClientBoundPacket for CSystemChatMessage {
    fn encode(&self) -> PacketEncoder {
        let mut buf = Vec::new();
        buf.write_text_component(&self.content);
        buf.write_bool(self.overlay);
        PacketEncoder::new(buf, play::SYSTEM_CHAT)
    }
}
