# Updating the minecraft version

This document explains the process of updating the minecraft version which can connect to MCHPRS.



### Updating mc_data

First we need to update the json files in mc_data by running minecraft's data generator, general instructions for this can be found here https://minecraft.wiki/w/Tutorial:Running_the_data_generator, but we will go into a specific example for MCHPRS.

Download a minecraft server of the specific version, make sure you download from http://piston-data.mojang.com. https://mcversions.net/ seems to give correct links but always double check you download from official servers.
```sh
mkdir java
cd java
curl https://piston-data.mojang.com/v1/objects/6bce4ef400e4efaa63a13d5e6f6b500be969ef81/server.jar -o server.jar
```

Run the data generator.

```sh
java -DbundlerMainClass="net.minecraft.data.Main" -jar server.jar --reports
```

Copy the relevant files to the mc_data directory.

```sh
cp generated/reports/blocks.json ../mc_data
cp generated/reports/registries.json ../mc_data
cp generated/reports/packets.json ../mc_data
```

Finally you can go back into the root directory.

```sh
cd ../
```

### Running the block data generator

Running the block data generator will produce the [generated.rs](../crates/blocks/src/generated.rs) file containing the `Block` and `Item` enums.

```sh
cd crates/block_data_gen
cargo run
```

### Update version constants
Lookup the correct values for and change `MC_VERSION`, `MC_DATA_VERSION` and `PROTOCOL_VERSION` in `crates/world/src/lib.rs`.

### TODO