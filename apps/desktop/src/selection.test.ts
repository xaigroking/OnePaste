import { describe, expect, it } from "vitest";
import { retainSelection, selectRow } from "./selection";
const order = ["a", "b", "c", "d"];
describe("history selection", () => {
  it("plain selection replaces the previous set", () => expect(selectRow({ ids: ["a", "b"], anchor: "a" }, order, "c")).toEqual({ ids: ["c"], anchor: "c" }));
  it("toggle adds and removes while retaining list order", () => {
    const selected = selectRow({ ids: ["c"], anchor: "c" }, order, "a", true);
    expect(selected.ids).toEqual(["a", "c"]); expect(selectRow(selected, order, "a", true).ids).toEqual(["c"]);
  });
  it("range replaces the set and keeps its anchor for shrinking", () => {
    const selected = selectRow({ ids: ["a", "b"], anchor: "b" }, order, "d", false, true);
    expect(selected).toEqual({ ids: ["b", "c", "d"], anchor: "b" }); expect(selectRow(selected, order, "c", false, true).ids).toEqual(["b", "c"]);
  });
  it("toggle plus range adds to the existing set", () => expect(selectRow({ ids: ["a", "c"], anchor: "c" }, order, "d", true, true).ids).toEqual(order.filter((id) => id !== "b")));
  it("removed rows are pruned and an absent anchor starts a new range", () => {
    expect(retainSelection({ ids: ["a", "c"], anchor: "c" }, ["a", "b"])).toEqual({ ids: ["a"], anchor: null });
    expect(selectRow({ ids: [], anchor: null }, order, "d", false, true)).toEqual({ ids: ["d"], anchor: "d" });
    expect(selectRow({ ids: ["a"], anchor: "missing" }, order, "d", false, true)).toEqual({ ids: ["d"], anchor: "d" });
  });
});

it("retains selection in the current list order after rows move", () => {
  expect(retainSelection({ ids: ["c", "a"], anchor: "c" }, order)).toEqual({ ids: ["a", "c"], anchor: "c" });
});
