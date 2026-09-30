import { Users } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useT } from "@/lib/i18n";
import { TRAE_POOL_STATUS_LABELS } from "@/lib/trae-gateway";
import type { TraeGatewayStatus } from "@/lib/trae-types";
import { cn } from "@/lib/utils";

/**
 * 「不指定」的哨兵值。
 *
 * Radix `Select` 不接受空字符串作为 `SelectItem` 的 value，而我们的契约里
 * **空串才表示「不指定」**（见 `TraeGatewayConfig.preferred_uid`），
 * 因此 UI 层用一个不可能与 uid 冲突的哨兵值承载它，在边界处转换。
 */
const AUTO_OPTION = "__auto__";

function formatCredits(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  return value.toLocaleString("zh-CN", { maximumFractionDigits: 2 });
}

function formatCount(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "0";
  return new Intl.NumberFormat("zh-CN").format(value);
}

/** 池五态计数格（与页面内联版逐字一致：可路由 / 冷却中 / 会话失效 / 积分过期 / 零积分）。 */
const POOL_TILES = [
  { key: "available", labelKey: "trae.gateway.pool.tile.available", tone: "ok" as const },
  { key: "cooling", labelKey: "trae.gateway.pool.tile.cooling", tone: "warn" as const },
  { key: "disabled", labelKey: "trae.gateway.pool.tile.disabled", tone: "danger" as const },
  { key: "expired", labelKey: "trae.gateway.pool.tile.expired", tone: "warn" as const },
  { key: "zeroCredits", labelKey: "trae.gateway.pool.tile.zeroCredits", tone: "muted" as const },
] as const;

/**
 * Trae 网关「账号池」卡（承接自 `TraeApiServicePage` 的内联区块）。
 *
 * 替代 WorkBuddy 的 `AccountStrategyCard`：Trae 无 `accountStrategy` 后端契约，
 * 只有 `pool` 五态计数、逐账号可路由状态，以及后端给出的 `diagnose` 排查串。
 *
 * 本组件基本是**纯展示**（状态由页面的 `trae_gateway_status` 传入），
 * 唯一例外是顶部的「指定账号」选择器 —— 它直接改写网关配置的 `preferred_uid`。
 *
 * ## 指定账号的语义（务必与后端一致）
 *
 * 它是**偏好而非约束**：指定账号不可用（冷却 / 禁用 / 零积分 / 已在本轮试过）时，
 * 网关会**回落到自动择优**，不会因为指定账号不可用而拒绝服务。
 * 因此这里不把「当前不可用」的账号禁用掉 —— 用户仍然可以指定它，
 * 等它恢复后自动生效。
 */
export function TraeAccountPoolCard({
  status,
  preferredUid,
  onPreferredUidChange,
  saving = false,
  className,
}: {
  status: TraeGatewayStatus | null;
  /** 当前指定的账号 uid；空串表示不指定。 */
  preferredUid: string;
  /** 选择变化时回调（空串 = 不指定）。 */
  onPreferredUidChange: (uid: string) => void;
  /** 保存中：禁用选择器，避免连点产生两次写入。 */
  saving?: boolean;
  className?: string;
}) {
  const t = useT();
  const pool = status?.pool;
  const accounts = status?.accounts ?? [];
  const preferredInPool = preferredUid !== "" && accounts.some((a) => a.uid === preferredUid);

  return (
    <Card className={cn("gap-0 py-0", className)}>
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border/60 px-5 py-3">
        <div className="flex items-center gap-2 text-sm font-semibold">
          <Users className="size-4 stroke-[1.75]" />
          {t("trae.gateway.pool.title")}
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <div className="flex items-center gap-2">
            <span className="text-xs text-muted-foreground">
              {t("trae.gateway.pool.preferredLabel")}
            </span>
            <Select
              value={preferredUid === "" ? AUTO_OPTION : preferredUid}
              onValueChange={(next) => onPreferredUidChange(next === AUTO_OPTION ? "" : next)}
              disabled={saving}
            >
              <SelectTrigger
                size="sm"
                className="w-48"
                aria-label={t("trae.gateway.pool.preferredAria")}
              >
                <SelectValue placeholder={t("trae.gateway.pool.preferredAuto")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={AUTO_OPTION}>{t("trae.gateway.pool.preferredAuto")}</SelectItem>
                {accounts.map((account) => (
                  <SelectItem key={account.uid} value={account.uid}>
                    {account.name}
                  </SelectItem>
                ))}
                {/* 指定的账号已不在池中（被删除 / 换了产品线）：仍要显示出来，
                    否则 Select 找不到匹配项会显示成空白，用户以为「没指定」。 */}
                {preferredUid !== "" && !preferredInPool && (
                  <SelectItem value={preferredUid}>
                    {t("trae.gateway.pool.preferredMissing")}
                  </SelectItem>
                )}
              </SelectContent>
            </Select>
          </div>
          <span className="text-xs text-muted-foreground">
            {t("trae.gateway.pool.totalRequests", { count: formatCount(status?.totalRequests ?? 0) })}
          </span>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-0 border-b border-border/60 sm:grid-cols-5">
        {POOL_TILES.map((item, index) => (
          <div
            key={item.key}
            className={cn(
              "flex flex-col items-center justify-center px-3 py-4 text-center",
              index > 0 && "border-l border-border/60",
            )}
          >
            <span className="text-xs text-muted-foreground">{t(item.labelKey)}</span>
            <span
              className={cn(
                "mt-2 text-2xl font-semibold tabular-nums tracking-[-0.02em]",
                item.tone === "ok"
                  ? "text-emerald-600 dark:text-emerald-400"
                  : item.tone === "warn"
                    ? "text-amber-600 dark:text-amber-400"
                    : item.tone === "danger"
                      ? "text-destructive"
                      : "text-muted-foreground",
              )}
            >
              {pool?.[item.key] ?? 0}
            </span>
          </div>
        ))}
      </div>

      {(status?.accounts.length ?? 0) === 0 ? (
        <p className="px-5 py-6 text-center text-sm text-muted-foreground">
          {t("trae.gateway.pool.empty")}
        </p>
      ) : (
        <div className="divide-y divide-border/60">
          {status?.accounts.map((account) => {
            const meta = TRAE_POOL_STATUS_LABELS[account.status];
            return (
              <div key={account.uid} className="flex flex-wrap items-center gap-3 px-5 py-3">
                <span className="min-w-0 flex-1 truncate text-sm font-medium">{account.name}</span>
                <span className="text-xs text-muted-foreground tabular-nums">
                  {t("trae.gateway.pool.credits", { credits: formatCredits(account.credits) })}
                </span>
                <Badge
                  variant={meta.tone === "danger" ? "destructive" : "secondary"}
                  className={cn(
                    "shrink-0",
                    meta.tone === "ok" && "text-emerald-600 dark:text-emerald-400",
                    meta.tone === "warn" && "text-amber-600 dark:text-amber-400",
                  )}
                >
                  {meta.label}
                </Badge>
                {account.cooldownReason && (
                  <span className="w-full truncate text-xs text-muted-foreground sm:w-auto">
                    {account.cooldownReason}
                  </span>
                )}
              </div>
            );
          })}
        </div>
      )}

      {(status?.diagnose.length ?? 0) > 0 && (
        <div className="border-t border-border/60 px-5 py-3">
          <p className="text-xs text-muted-foreground">
            {t("trae.gateway.pool.diagnoseNote")}
          </p>
          <ul className="mt-2 space-y-1">
            {status?.diagnose.map((line) => (
              <li key={line} className="break-all font-mono text-xs text-muted-foreground">
                {line}
              </li>
            ))}
          </ul>
        </div>
      )}
    </Card>
  );
}
