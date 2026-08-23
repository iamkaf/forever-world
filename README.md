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

Forever World 1.2.0 is for Minecraft 26.2 with Fabric Loader 0.19.3. Use Java 25 in the launcher.

## Play

Forever World 1.2.0 is available from GitHub Releases, [Modrinth](https://modrinth.com/modpack/kafs-forever-world), [CurseForge](https://www.curseforge.com/minecraft/modpacks/forever-world), and the [Maven repository](https://maven.kaf.sh/com/iamkaf/modpacks/forever-world/1.2.0/forever-world-1.2.0-client.mrpack). Import the client `.mrpack` in Prism or whatever else eats Modrinth packs. Complementary Unbound is already in the instance. The CurseForge edition has 47 of the pack's 48 entries because Presence Footsteps has no Minecraft 26.2 file on CurseForge. The other editions contain all 48.

## Host

For a persistent dedicated server, use [Pastel](https://kaf.sh/pastel). It verifies the server files, installs Fabric and Java, and keeps Minecraft running. Pastel is for running the published pack.

From an empty server directory:

```bash
curl -fsSL https://kaf.sh/pastel/install.sh | sh
./pastel install com.iamkaf.modpacks:forever-world:1.2.0 -repo https://maven.kaf.sh
./pastel run
```

Pastel leaves client-only files off the dedicated server. Running the server means you agree to [Minecraft's EULA](https://aka.ms/MinecraftEULA).

Want to change the pack or run it from source? See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Forever World's original source is licensed under the [PolyForm Shield License 1.0.0](LICENSE). Read the license before redistributing the repository or using it in another product.
