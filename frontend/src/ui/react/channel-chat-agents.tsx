import { Button, Dialog, Status } from "@sproyt/ui/react";
import { useEffect, useState } from "react";
import type { ChannelChatAgents, CircleChatAgentApi } from "../../chat-agents";

export function ChannelChatAgentsDialog({ api, channelId, channelName, privateChannel, onClose }: {
  api: CircleChatAgentApi; channelId: string; channelName: string; privateChannel: boolean; onClose: () => void;
}) {
  const [state, setState] = useState<ChannelChatAgents>();
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);
  const [reload, setReload] = useState(0);
  useEffect(() => {
    let live = true;
    setError(""); setState(undefined);
    void api.listChannel(channelId).then(value => { if (live) setState(value); })
      .catch(cause => { if (live) setError(cause instanceof Error ? cause.message : "Kunne ikkje hente agentval."); });
    return () => { live = false; };
  }, [api, channelId, reload]);
  const select = async (agentId: string, enabled: boolean) => {
    if (!state || pending) return;
    setPending(true); setError("");
    try { setState(await api.selectChannel(channelId, agentId, enabled, state.accessRevision)); }
    catch (cause) { setError(cause instanceof Error ? cause.message : "Kunne ikkje lagre agentval. Hent vala på nytt før du prøver igjen."); }
    finally { setPending(false); }
  };
  return <Dialog open title={`Agentar i ${channelName}`} closeLabel="Lukk kanalagentar" onClose={onClose}>
    <p>{privateChannel
      ? "Private kanalar har ingen agentar før du vel dei her. Ein vald agent får lese dei siste 20 minutta i denne kanalen eller tråden når han blir utløyst."
      : "Vel kva kretsagentar som får svare her. Kretsen styrer oppsettet; du styrer tilgangen til denne kanalen."}</p>
    {state ? <div className="sp-channel-agent-list">
      {!state.selectionAvailable && <><Status>Kanalval blir aktivert når utrullinga er ferdig.</Status><Button onClick={() => setReload(value => value + 1)}>Hent vala på nytt</Button></>}
      {state.agents.length === 0 && <Status>Ingen agentar i kretsen enno.</Status>}
      {state.agents.map(agent => <label key={agent.agentId} className="sp-channel-agent-choice">
        <input type="checkbox" checked={agent.enabled} disabled={pending || !!error || !state.selectionAvailable} onChange={event => void select(agent.agentId, event.currentTarget.checked)} />
        <span>{agent.displayName}{!agent.agentEnabled && <small>Avslått i kretsen</small>}</span>
      </label>)}
    </div> : !error && <Status>Hentar agentval …</Status>}
    {pending && <Status>Lagrar agentval …</Status>}
    {error && <><Status tone="error">{error}</Status><Button disabled={pending} onClick={() => setReload(value => value + 1)}>Hent vala på nytt</Button></>}
    <p className="sp-help">Når du slår av tilgang, blir ventande svar stoppa. Svar som allereie er sende, blir ståande.</p>
  </Dialog>;
}
