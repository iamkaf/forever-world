import { Capability, Readiness, describe, expect, test } from "@teakit/test";

const REQUIRED_GAMEPLAY_CONTENT = [
  "kafvalentine:aristea",
  "kafvalentine:cotton_candy_block",
  "kafvalentine:lovey_dovey_infuser",
  "bonded:repair_bench",
  "bonded:tool_bench",
  "mochila:black_leather_backpack",
];

describe.configure({
  timeout: "3m",
  readiness: [Readiness.World, Readiness.Player],
  capabilities: [Capability.RegistryLookup],
});

describe("Startup", () => {
  test("connects the client to the dedicated server", async (ctx) => {
    const info = await ctx.session.info();
    expect(info.paired).toBe(true);
    expect(info.client).toBe(true);
    expect(info.server).toBe(true);

    const [client, server] = await Promise.all([
      ctx.session.client.health(),
      ctx.session.server.health(),
    ]);
    expect(client.loaderError?.active ?? false).toBe(false);
    expect(server.status?.worldLoaded).toBe(true);
    expect(server.status?.playerCount ?? 0).toBeGreaterThan(0);
  });

  test("loads the pack's gameplay content", async (ctx) => {
    const missing = await ctx.registry.missing(REQUIRED_GAMEPLAY_CONTENT);
    expect(missing).toEqual([]);
  });
});
