/* Derived from wb-switch at 22b318c1f44b335c2222e444abcc2d0f74e160f7.
 * MIT license: crates/buddy-switch-core/src/modules/SESSION-LINKS-MIT.txt. */
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { FileText } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { useT } from "@/lib/i18n";
import type { SessionGroupDetail, SessionGroupMemberDetail } from "@/lib/types";

import "./session-group-detail.css";

type GraphLine = { path: string; tone: "default" | "common" | `branch-${number}`; points: { x: number; y: number }[] };

export function SessionRelationshipGraph({ detail, renderMember }: {
  detail: SessionGroupDetail;
  renderMember: (member: SessionGroupMemberDetail, common: boolean, branchIndex?: number) => ReactNode;
}) {
  const t = useT();
  const graphRef = useRef<HTMLDivElement>(null);
  const [connections, setConnections] = useState<GraphLine[]>([]);
  useLayoutEffect(() => {
    const graph = graphRef.current;
    if (!graph) return;
    function measure() {
      if (!graph) return;
      const svg = graph.querySelector("svg");
      const matrix = svg?.getScreenCTM();
      if (!matrix) return;
      // 屏幕坐标包含弹窗缩放；还原到 SVG 坐标，连线才不会被二次缩放。
      const inverse = matrix.inverse();
      const toLocal = (x: number, y: number) => new DOMPoint(x, y).matrixTransform(inverse);
      const hub = graph.querySelector<HTMLElement>("[data-session-hub]")?.getBoundingClientRect();
      if (!hub) return;
      const nodes = [...graph.querySelectorAll<HTMLElement>("[data-session-member]")];
      const lineage = detail?.summaryStatus === "diverge" && detail.divergence;
      const hasLineage = !!lineage && nodes.every((node) => node.dataset.graphRole === "common" || node.dataset.graphRole === "branch");
      const leftCount = lineage ? lineage.commonMemberIds.length : Math.ceil(nodes.length / 2);
      const next = nodes.map((node, index) => {
        const box = node.getBoundingClientRect();
        const left = index < leftCount;
        const sideCount = left ? leftCount : nodes.length - leftCount;
        const sideIndex = left ? index : index - leftCount;
        const { x, y } = toLocal(left ? box.right : box.left, box.top + box.height / 2);
        const { x: hx, y: hy } = toLocal(left ? hub.left : hub.right, hub.top + hub.height * ((sideIndex + 1) / (sideCount + 1)));
        const mid = (x + hx) / 2;
        const dx = Math.sign(hx - x);
        const dy = Math.sign(hy - y);
        const radius = Math.min(12, Math.abs(hy - y) / 2, Math.abs(hx - x) / 4);
        const path = hasLineage && Math.abs(hy - y) > 1
          ? `M ${x} ${y} C ${mid} ${y} ${mid} ${hy} ${hx} ${hy}`
          : radius < 1 ? `M ${x} ${y} H ${hx}`
            : `M ${x} ${y} H ${mid - dx * radius} Q ${mid} ${y} ${mid} ${y + dy * radius} V ${hy - dy * radius} Q ${mid} ${hy} ${mid + dx * radius} ${hy} H ${hx}`;
        const tone = hasLineage ? left ? "common" as const : `branch-${Number(node.dataset.branchIndex ?? 0) % 5}` as const : "default" as const;
        return { path, tone, points: [{ x, y }, { x: hx, y: hy }] };
      });
      setConnections(next);
    }
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(graph);
    graph.querySelectorAll<HTMLElement>("[data-session-member], [data-session-hub]").forEach((node) => observer.observe(node));
    measure();
    return () => observer.disconnect();
  }, [detail]);

  const divergence = detail.summaryStatus === "diverge" ? detail.divergence : undefined;
  const common = new Set(divergence?.commonMemberIds);
  const branches = new Map(divergence?.branches.flatMap((branch, index) =>
    branch.map((id) => [id, index] as const)));
  const members = divergence
    ? [...detail.members].sort((a, b) => Number(common.has(b.memberId)) - Number(common.has(a.memberId)))
    : detail.members;
  const leftCount = divergence ? common.size : Math.ceil(members.length / 2);
  const heading = t(divergence ? "sessions.workbench.lineage" : "sessions.workbench.relationship");
  return (
    <section aria-label={heading} className="space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-semibold">{heading}</h3>
        {divergence && <Badge variant="warning">{t("sessions.workbench.branchCount", { count: divergence.branches.length })}</Badge>}
      </div>
      <div className="relationship-canvas rounded-xl border border-brand/15 bg-brand/5 p-3">
        <div ref={graphRef} className="relationship-graph" data-lineage={!!divergence}>
          <svg className="relationship-connections" aria-hidden="true">
            {connections.map((line, index) => (
              <g key={index} data-line-tone={line.tone}>
                <path d={line.path} />
                {line.points.map((point, pointIndex) => <circle key={pointIndex} cx={point.x} cy={point.y} r="3.5" />)}
              </g>
            ))}
          </svg>
          <div className="relationship-root" data-session-hub>
            <FileText className="size-5 text-brand" aria-hidden="true" />
            <p className="text-sm font-semibold">{t(divergence ? "sessions.workbench.common" : "sessions.workbench.sameSession")}</p>
            <p className="text-xs text-muted-foreground">
              {t("sessions.workbench.members", { count: divergence ? common.size : members.length })}
            </p>
          </div>
          <ul className="relationship-members" aria-label={heading}>
            {members.map((member, index) => {
              const isCommon = common.has(member.memberId);
              const branchIndex = branches.get(member.memberId);
              const left = index < leftCount;
              return (
                <li
                  key={member.memberId}
                  className="relationship-branch"
                  style={{ gridColumn: left ? 1 : 3, gridRow: left ? index + 1 : index - leftCount + 1 }}
                  data-session-member={member.memberId}
                  data-graph-role={isCommon ? "common" : branchIndex !== undefined ? "branch" : undefined}
                  data-branch-index={branchIndex === undefined ? undefined : branchIndex % 5}
                >
                  {renderMember(member, isCommon, branchIndex)}
                </li>
              );
            })}
          </ul>
        </div>
      </div>
      {divergence && <p className="text-xs text-muted-foreground">{t("sessions.workbench.branchHint")}</p>}
    </section>
  );
}
