# Multi-version Minecraft-compatible client written in Rust

## Version Support

Uses a multi-protocol system to allow connection to various Minecraft versions.

| Game Version | Protocol Version | Supported? |
| --- | --- | --- |
| 1.16.5 | 754 | Yes (Not 100%) |
| 1.16.4 | 754 | Yes (Not 100%) |
| 1.16.3 | 753 | Yes (Not 100%) |
| 1.16.2 | 751 | Yes (Not 100%) |
| 1.16.1 | 736 | Yes (Not 100%) |
| 1.16 | 735 | Yes (Not 100%) |
| 1.15.2 | 578 | Yes (Not 100%) |
| 1.15.1 | 575 | Yes (Not 100%) |
| 1.14.4 | 498 | Yes (Not 100%) |
| 1.14.3 | 490 | Yes (Not 100%) |
| 1.14.2 | 485 | Yes (Not 100%) |
| 1.14.1 | 480 | Yes (Not 100%) |
| 1.14 | 477 | Yes (Not 100%) |
| 1.13.2 | 404 | Yes (Not 100%) |
| 1.12.2 | 340 | Yes (Not 100%) |
| 1.11.2 | 316 | Yes (Not 100%) |
| 1.11 | 315 | Yes (Not 100%) |
| 1.10.2 | 210 | Yes (Not 100%) |
| 1.9.2 | 109 | Yes (Not 100%) |
| 1.9 | 107 | Yes (Not 100%) |
| 1.8.9 | 47 | Yes (Not 100%) |
| 1.7.10 | 5 | Yes (Not 100%) |

---

## Feature Progress

![Progress](https://img.shields.io/badge/Progress-10%2F129%20(7.7%25)-blue?style=for-the-badge)

### Major Features

| Feature | Status | Notes |
| --- | --- | --- |
| **Hitting Entities** | ✓ | Basic entity interaction mechanics |
| **1.16.5 Textures** | ✓ | Updated texture asset support |
| **Sprinting** | ✗ | Movement speed modification |
| **Crouching / Sneaking** | ✗ | Height reduction and edge detection |
| **Swimming Mechanics** | ✗ | Water physics and swim state |
| **Water Interaction** | ✗ | Buoyancy and drag calculations |
| **Oxygen System** | ✗ | Underwater breath meter and damage |
| **Zombie Animations** | ✗ | Animation handling for zombies |
| **Player Physics Fixes** | ✗ | Movement parity with vanilla client |

---

### Entities Support

| Category | Entities Included | Status |
| --- | --- | --- |
| **Hostile Mobs** | **Zombie** | **✓** |
|  | Creeper, Spider, Cave Spider, Slime, Ghast, Zombified Piglin, Enderman, Magma Cube, Silverfish, Endermite, Witch, Guardian, Elder Guardian, Shulker, Husk, Stray, Zombie Villager, Phantom, Drowned, Pillager, Ravager, Vex, Evoker, Vindicator, Illusioner, Hoglin, Piglin Brute, Zoglin | ✗ |
| **Passive Mobs** | Pig, Sheep, Cow, Chicken, Squid, Mooshroom, Rabbit, Polar Bear, Fox, Bee, Panda, Turtle, Dolphin, Cod, Salmon, Pufferfish, Tropical Fish, Cat, Ocelot, Parrot, Wandering Trader | ✗ |
| **Neutral Mobs** | Wolf, Iron Golem, Snow Golem, Strider | ✗ |
| **Mounts & Tames** | Horse, Donkey, Mule, Skeleton Horse, Llama, Trader Llama | ✗ |
| **Bosses** | Ender Dragon, Wither | ✗ |
| **Projectiles & Thrown** | Arrow, Tipped Arrow, Spectral Arrow, Snowball, Egg, Fireball, Small Fireball, Dragon Fireball, Wither Skull, Ender Pearl, Eye of Ender, XP Bottle, Splash/Lingering Potion, Trident, Shulker Bullet, Llama Spit, Fishing Hook | ✗ |
| **Vehicles & Interactive** | Boat, Minecart (Standard, Chest, Furnace, TNT, Hopper, Command Block, Spawner), Armor Stand | ✗ |
| **World & Objects** | Primed TNT, Falling Blocks (Sand, Gravel, Anvil), Painting, Item Frame, Experience Orb, Lead Knot, Lightning, Area Effect Cloud, Ender Crystal, Evoker Fangs | ✗ |

---

### Minor Features & UI

| Feature | Status | Notes |
| --- | --- | --- |
| **Raw Mouse Look** | ✓ | Camera movement directly matches mouse input |
| **Player Self-Render Fix** | ✓ | Hides first-person body model when looking down |
| **Block Break Overlay** | ✓ | Crack textures and progressive break rendering |
| **3D Inventory Items** | ✓ | Inventory renders 3D block models |
| **Death Messages** | ✓ | Chat output on player death |
| **Scroll Selection** | ✓ | Mouse wheel hotbar navigation |
| **FPS Counter** | ✓ | Displayed in top-right corner |
| **Durability Bar** | ✗ | Visual item wear indicator |
| **First-Person Hand Rendering** | ✗ | Render hand holding tools, weapons, shields, buckets & blocks |
| **Item Entities** | ✗ | Ground item stack rendering & rotation |
| **Inventory Mechanics** | ✗ | Dragging items, crafting grid auto-updates, avatar view |
| **Controls & Settings** | ✗ | Keybindings remapping & FOV slider |
| **Environment & Overlays** | ✗ | Fire overlay, particles, shadow fixes, totem animation |
| **Audio System** | ✗ | Sound effects and music engine |
| **Commands & Chat** | ✗ | `/` command bar trigger, syntax highlighting, parsing |
| **HUD Elements** | ✗ | Experience bar, off-hand slot indicator |
| **Creative Mode HUD** | ✗ | Dedicated creative tab interface |
| **Cosmetics & Packs** | ✗ | Capes support and resource pack loading |
| **PumpkinMC Integration** | ✗ | Interoperability with PumpkinMC Rust ecosystem |

---

## Credits

Forked from [Leafish](https://github.com/Lea-fish/Leafish), which originates from [Stevenarella](https://github.com/iceiix/stevenarella) by [@iceiix](https://github.com/iceiix) which originates from [Steven](https://github.com/thinkofname/steven) by [@Thinkofname](https://github.com/thinkofname) ported from [Steven-go](https://github.com/Thinkofname/steven-go) by [@Thinkofname](https://github.com/Thinkofname)

---

## Building

Requires **Rust 1.53.0** or newer.

Run from root directory:

```sh
# Compile and run directly
cargo run --release

# Build release executable only
cargo build --release

```

---

## License

Dual-licensed under [MIT](https://www.google.com/search?q=LICENSE-MIT) and [Apache 2.0](https://www.google.com/search?q=LICENSE-APACHE).
