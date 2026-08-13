import { Capability, Readiness, describe, expect, test } from "@teakit/test";

const CLUSTER_IDS = [
  "kafvalentine:aristea",
  "bonded:repair_bench",
  "mochila:black_leather_backpack",
];

describe.configure({
  timeout: "3m",
  readiness: [Readiness.World, Readiness.Player],
  capabilities: [
    Capability.PlayerPosition,
    Capability.PlayerTeleport,
    Capability.WorldBlock,
    Capability.WorldSetBlock,
  ],
});

describe("Forever World pair", () => {
  test("joins the dedicated server with the Kaf cluster loaded", async (ctx) => {
    const info = await ctx.session.info();
    expect(info.paired).toBe(true);
    expect(info.client).toBe(true);
    expect(info.server).toBe(true);

    const health = await ctx.session.server.health();
    expect(health.status?.worldLoaded).toBe(true);
    expect(health.status?.playerCount ?? 0).toBeGreaterThan(0);

    const missing = await ctx.registry.missing(CLUSTER_IDS);
    expect(missing).toEqual([]);

    await ctx.player.teleport({ x: 8.5, y: 80, z: 8.5 });
    await ctx.world.setBlock({ x: 8, y: 79, z: 8 }, "minecraft:stone");
    const block = await ctx.world.block({ x: 8, y: 79, z: 8 });
    expect(block.id).toBe("minecraft:stone");
  });
});
