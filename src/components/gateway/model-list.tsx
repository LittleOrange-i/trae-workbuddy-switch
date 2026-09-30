import { useState } from "react";
import { Loader2, RefreshCw } from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { DemoAction } from "@/components/demo-action";
import * as api from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import type { TranslationKey } from "@/locales/zh";
import type { CatalogSource, Region } from "@/lib/types";
import { useGatewayStore } from "@/stores/gateway";

/** 只存键不存文案：语言切换时整张表才会跟着变。 */
const SOURCE_LABEL: Record<CatalogSource, { labelKey: TranslationKey; variant: "success" | "secondary" | "warning" }> = {
  live: { labelKey: "wbStats.gateway.sourceLive", variant: "success" },
  cached: { labelKey: "wbStats.gateway.sourceCached", variant: "secondary" },
  builtin: { labelKey: "wbStats.gateway.sourceBuiltin", variant: "warning" },
};

function formatTime(ts: number | null): string {
  if (!ts) return "—";
  const date = new Date(ts);
  if (Number.isNaN(date.getTime())) return "—";
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

/**
 * 模型列表：展示来源徽标（实时 / 已保存 / 内置）与刷新按钮（P0-4 / P0-12 / P1-7）。
 *
 * ## 版本由页面传入（受控），组件内**不再自带版本选择器**
 *
 * 改造前这里有个内部 `Tabs`，与页面上的接入地址 / 账号池 / 接入指引各自为政：
 * 在这张卡里切到国际版，页面其他部分还停在国内版 —— 同一个「当前版本」在四个地方
 * 各说各话。现在与 Trae 侧一致：**页面持有唯一版本，本组件只负责渲染**。
 */
export function ModelList({ region, className }: { region: Region; className?: string }) {
  const t = useT();
  const [refreshing, setRefreshing] = useState(false);
  const snapshot = useGatewayStore((s) => s.models[region]);
  const refreshModels = useGatewayStore((s) => s.refreshModels);

  async function onRefresh() {
    setRefreshing(true);
    try {
      await refreshModels(region);
      toast.success(t("wbStats.gateway.refreshed"));
    } catch (e) {
      toast.error(t("wbStats.gateway.refreshFail"), { description: api.asError(e) });
    } finally {
      setRefreshing(false);
    }
  }

  const source = snapshot ? SOURCE_LABEL[snapshot.source] : null;

  return (
    <Card className={cn("gap-0 py-0", className)}>
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border/60 px-5 py-3">
        {/* 版本不在这里显示也不在这里切：页头切换器是唯一入口（与 Trae 侧同口径）。 */}
        <span className="text-sm font-semibold">{t("wbStats.gateway.modelList")}</span>
        <DemoAction>
          <Button variant="ghost" size="sm" onClick={() => void onRefresh()} disabled={refreshing}>
            {refreshing ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            {t("wbStats.gateway.refresh")}
          </Button>
        </DemoAction>
      </div>

      <div className="px-5 py-4">
        {source && (
          <div className="mb-3 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
            <span className="flex items-center gap-1.5">
              {t("wbStats.gateway.source")}
              <Badge variant={source.variant} className="rounded-md">
                {t(source.labelKey)}
              </Badge>
            </span>
            <span>{t("wbStats.gateway.updatedAt", { time: formatTime(snapshot?.fetched_at ?? null) })}</span>
            <span>{t("wbStats.gateway.modelsCount", { n: snapshot?.models.length ?? 0 })}</span>
          </div>
        )}
        {snapshot?.note && <p className="mb-3 text-xs text-amber-600">{snapshot.note}</p>}
        {!snapshot || snapshot.models.length === 0 ? (
          <p className="py-4 text-sm text-muted-foreground">{t("wbStats.gateway.noModels")}</p>
        ) : (
          <div className="flex flex-wrap gap-2">
            {snapshot.models.map((model) => (
              <span
                key={model.id}
                className="inline-flex items-center gap-1.5 rounded-lg border border-border bg-muted/40 px-2.5 py-1 text-xs"
                title={`${t("wbStats.gateway.modelTitle", {
                  name: model.name,
                  context: model.context_window,
                  max: model.max_tokens,
                })}${model.credits ? ` · ${model.credits}` : ""}`}
              >
                <span className="font-medium">{model.name}</span>
                {model.free && (
                  <Badge variant="success" className="rounded-md px-1.5 py-0 text-[10px]">
                    {t("wbStats.gateway.free")}
                  </Badge>
                )}
                {model.badges.map((badge) => (
                  <Badge key={badge} variant="warning" className="rounded-md px-1.5 py-0 text-[10px]">
                    {badge}
                  </Badge>
                ))}
              </span>
            ))}
          </div>
        )}
      </div>
    </Card>
  );
}
