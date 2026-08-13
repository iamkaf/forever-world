# Forever World

![Two players in diamond armor sitting on a wooden deck at night while a firework bursts overhead.](images/banner.png)

This world does not get thrown away.

Not when the next Minecraft version ships. Not because a shader looks dated. Same chunks, same mistakes, same house. The pack exists so that save still loads.

That is a fucking promise. I can only make it for mods I will still be here to port. So the only content allowed to live in the server's chunks is mine: [Liteminer](https://modrinth.com/mod/liteminer), [Bonded](https://modrinth.com/mod/bonded), [Mochila](https://modrinth.com/mod/mochila), [Torch Toss](https://modrinth.com/mod/torch-toss), [SnapShears](https://modrinth.com/mod/snapshears), [Valentine](https://modrinth.com/mod/kafs-valentine-special), [Kaf HUD](https://modrinth.com/mod/kaf-hud), [Happy Ghast Improvements](https://modrinth.com/mod/happyghastimprovements), [Gentle Hurt Cam](https://modrinth.com/mod/gentlehurtcam). Amber and Konfig because those need them. A backpack in a chest, a repair bench, Aristea in the dirt. The world remembers those, so I have to remember them too. As long as forever is.

![Liteminer selecting a small tunnel through deepslate.](images/liteminer.webp)

![Bonded's tool bench overlay on a pickaxe.](images/bonded.webp)

![A row of Mochila backpacks.](images/mochila.webp)

![A potted Aristea on a shelf.](images/valentine.webp)

![A mushroom house in a valley with wheat fields, roses, and a campfire.](images/house.webp)

Sodium, Iris, Lithium, the sound mods, the shader folder, C2ME, JEI, all of that can come and go. The save does not know they were there. If they vanish we change the pack and keep playing. I will not put anyone else's blocks in this world. I cannot promise those still exist.

![Two players sitting on a garden bench by a campfire.](images/together.webp)

Current pack is 1.1.1. Minecraft 26.2, Fabric Loader 0.19.3. Java 25 in the launcher.

## Play

Import the [mrpack](https://maven.kaf.sh/com/iamkaf/modpacks/forever-world/1.1.1/forever-world-1.1.1.mrpack) in Prism or whatever else eats Modrinth packs. Open Iris and pick a shader. They're all already in the instance.

## Host

[Pastel](https://kaf.sh/pastel) lives in the server folder. From an empty directory:

```bash
curl -fsSL https://kaf.sh/pastel/install.sh | sh
./pastel install com.iamkaf.modpacks:forever-world:1.1.1 -repo https://maven.kaf.sh
./pastel run
```

Pastel leaves the client-only stuff off the dedicated server. It'll fetch Java if the machine doesn't already have something new enough. Running the server means you agree to [Minecraft's EULA](https://aka.ms/MinecraftEULA).

## Building it

This repo is the source for `com.iamkaf.modpacks:forever-world`. `pack.toml` pins every download. The `pack` tool fetches those pins, writes `pack.lock.toml`, and exports the `.mrpack`.

```bash
just check
just export
just verify
```

1.1.1 is already on Maven. `just verify` checks that an export still matches that artifact. Don't overwrite it.

`just pastel-install` sets up a Pastel server in `server/` from the export. `just pair` boots a dedicated server and a client together and runs `test/teakit/pair-smoke.test.ts`. TeaKit is only for that test. It never goes in the pack, and it doesn't bump Fabric Loader off 0.19.3.

## License

Source available, all rights reserved. You can read this. You can't copy it, publish it, or ship it as your own without asking. See [LICENSE](LICENSE).

The mods and shaderpacks keep their own licenses. I'm not relicensing Sodium.
