import { describe, expect, it } from "vitest";

import { qoderPacksPrefetchFingerprint } from "@/lib/qoder-packs";

describe("qoderPacksPrefetchFingerprint", () => {
  it("changes when accounts are added or replaced even when the list length stays the same", () => {
    const before = qoderPacksPrefetchFingerprint("qoder-cn", ["alice", "bob"]);
    const added = qoderPacksPrefetchFingerprint("qoder-cn", ["alice", "bob", "carol"]);
    const replaced = qoderPacksPrefetchFingerprint("qoder-cn", ["alice", "dave"]);

    expect(added).not.toBe(before);
    expect(replaced).not.toBe(before);
  });

  it("changes when the active Qoder scope changes", () => {
    expect(qoderPacksPrefetchFingerprint("qoder-cn", ["alice"])).not.toBe(
      qoderPacksPrefetchFingerprint("qoder-global", ["alice"]),
    );
  });

  it("stays stable when the same account set is reordered", () => {
    expect(qoderPacksPrefetchFingerprint("qoder-cn", ["alice", "bob"])).toBe(
      qoderPacksPrefetchFingerprint("qoder-cn", ["bob", "alice"]),
    );
  });
});
