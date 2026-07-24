import { describe, it, expect } from "vitest";
import { applyAssignment } from "./mapAssignment";
import type { MapPoint, MapProject } from "../../types/embeddingMap";

const point = (mailId: string): MapPoint => ({
  x: 0,
  y: 0,
  mail_id: mailId,
  subject: `件名${mailId}`,
  project_id: null,
  project_name: null,
  project_color: null,
});

const project: MapProject = { id: "p1", name: "案件A", color: "#ff0000" };

describe("applyAssignment", () => {
  it("該当する点に案件ラベルと色を反映する", () => {
    const next = applyAssignment([point("m1"), point("m2")], "m1", project);
    expect(next[0].project_id).toBe("p1");
    expect(next[0].project_name).toBe("案件A");
    expect(next[0].project_color).toBe("#ff0000");
  });

  it("他の点は変更しない", () => {
    const next = applyAssignment([point("m1"), point("m2")], "m1", project);
    expect(next[1].project_id).toBeNull();
  });

  it("該当が無ければ配列内容は変わらない", () => {
    const points = [point("m1")];
    const next = applyAssignment(points, "unknown", project);
    expect(next).toEqual(points);
  });
});
