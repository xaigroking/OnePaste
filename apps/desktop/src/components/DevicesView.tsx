import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import type { AppStatus, DeviceList, DeviceView } from "../api";
import { formatRelative } from "../format";
import { Icon } from "./Icon";
import { Badge, Button, Card } from "./ui";

/** The devices of the vault: who is in it, when they were last seen, removing one, and adding another. */
export function DevicesView({ status, onCount }: { status: AppStatus | null; onCount: (n: number) => void }) {
  const { t, i18n } = useTranslation();
  const [list, setList] = useState<DeviceList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [code, setCode] = useState<string | null>(null);
  const synced = status?.mode === "synced";

  const load = useCallback(async () => {
    try {
      const l = await api.devices();
      setList(l);
      setError(null);
      onCount(l.devices.filter((d) => !d.revoked).length);
    } catch (e) {
      setError(String(e));
    }
  }, [onCount]);

  useEffect(() => {
    if (!synced) return;
    void load();
    const timer = window.setInterval(() => void load(), 15000);
    return () => window.clearInterval(timer);
  }, [synced, load]);

  if (status && !synced) {
    return (
      <div className="page">
        <h1>{t("nav.devices")}</h1>
        <div className="empty">
          <Icon name="monitor" size={28} />
          <strong>{t("devices.localTitle")}</strong>
          <span>{t("devices.localBody")}</span>
        </div>
      </div>
    );
  }

  const remove = async (d: DeviceView) => {
    try {
      await api.revokeDevice(d.deviceId);
      setRemoving(null);
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  const makeCode = async () => {
    try {
      setCode(await api.newConnectionCode());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const seen = (d: DeviceView) => (d.thisDevice ? t("devices.now") : d.lastSeenAt ? formatRelative(d.lastSeenAt * 1000, i18n.language) : t("devices.never"));

  return (
    <div className="page">
      <h1>{t("nav.devices")}</h1>
      {error && (
        <p className="notice notice-danger" role="alert">
          {t(`devices.errors.${error}`, { defaultValue: t("devices.errors.other", { code: error }) })}
        </p>
      )}
      <Card title={t("devices.listTitle")}>
        {!list ? (
          <p className="muted">{t("devices.loading")}</p>
        ) : (
          <table className="devices">
            <thead>
              <tr>
                <th>{t("devices.colName")}</th>
                <th>{t("devices.colSeen")}</th>
                <th>{t("devices.colState")}</th>
                <th>{t("devices.colActions")}</th>
              </tr>
            </thead>
            <tbody>
              {list.devices.map((d) => (
                <tr key={d.deviceId}>
                  <td>
                    <span className="row-icon">
                      <Icon name="monitor" />
                    </span>
                    <span className="device-name">
                      {d.name ?? t("devices.unreadable")}
                      {d.thisDevice && <Badge tone="ok">{t("devices.thisBadge")}</Badge>}
                    </span>
                  </td>
                  <td className="muted">{seen(d)}</td>
                  <td>
                    <Badge tone={d.revoked ? "danger" : "ok"} icon={<Icon name={d.revoked ? "alert" : "check"} size={13} />}>
                      {d.revoked ? t("devices.removed") : t("devices.active")}
                    </Badge>
                  </td>
                  <td className="actions-cell">
                    {!d.revoked && (
                      <>
                        {/* The server keeps the name as it was first sent, so renaming has to wait for a server change */}
                        <Button disabled title={t("devices.soonHint")}>
                          {t("devices.rename")} · {t("devices.soon")}
                        </Button>
                        {!d.thisDevice && <Button variant="danger" onClick={() => setRemoving(d.deviceId)}>{t("devices.remove")}</Button>}
                      </>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {list &&
          removing &&
          list.devices
            .filter((d) => d.deviceId === removing)
            .map((d) => (
              <div key={d.deviceId} className="confirm" role="alertdialog" aria-label={t("devices.removeTitle", { name: d.name ?? "" })}>
                <p className="notice notice-warn">{t("devices.removeBody", { name: d.name ?? t("devices.unreadable") })}</p>
                <div className="setting">
                  <Button variant="danger" onClick={() => void remove(d)}>
                    {t("devices.removeConfirm")}
                  </Button>
                  <Button onClick={() => setRemoving(null)}>{t("security.cancel")}</Button>
                </div>
              </div>
            ))}
      </Card>

      <Card title={t("devices.addTitle")}>
        <p className="muted">{t("devices.addBody")}</p>
        {code === null ? (
          <Button variant="primary" onClick={() => void makeCode()}>
            {t("devices.addMake")}
          </Button>
        ) : (
          <>
            <pre className="kit" data-testid="connection-code">{code}</pre>
            <p className="notice notice-warn">{t("devices.addReminder")}</p>
            <div className="setting">
              <Button onClick={() => void api.copySecret(code)}>{t("onboarding.kit.copy")}</Button>
              <Button onClick={() => setCode(null)}>{t("devices.addDone")}</Button>
            </div>
          </>
        )}
      </Card>

      {list && <p className="muted small">{t("devices.footer", { server: list.serverUrl })}</p>}
    </div>
  );
}
