import { Button, Dialog, Status } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";
import type { ChannelChatAgents, CircleChatAgentApi } from "../../chat-agents";

export function ChannelChatAgentsDialog({ api, channelId, channelName, privateChannel, onClose }: {
  api: CircleChatAgentApi; channelId: string; channelName: string; privateChannel: boolean; onClose: () => void;
}) {
  return <Dialog open title={`Agentar i ${channelName}`} closeLabel="Lukk kanalagentar" onClose={onClose}>
    <ChannelChatAgentsPanel api={api} channelId={channelId} privateChannel={privateChannel} />
  </Dialog>;
}

/** Shared by the channel menu and circle setup; the server remains the access authority. */
export function ChannelChatAgentsPanel({ api, channelId, privateChannel, agentId, admitted = () => true, onOpenAgent }: {
  api: CircleChatAgentApi; channelId: string; privateChannel: boolean; agentId?: string; admitted?: () => boolean; onOpenAgent?: (agentId: string) => void;
}) {
  const [state, setState] = useState<ChannelChatAgents>();
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);
  const [reload, setReload] = useState(0);
  const guard = useRef(admitted); guard.current = admitted;
  const lifecycle = useRef(0), mutationPending = useRef(false);
  useEffect(() => {
    let live = true;
    const epoch = ++lifecycle.current;
    const allowed = admitted;
    mutationPending.current = false; setPending(false);
    setError(""); setState(undefined);
    void api.listChannel(channelId).then(value => { if (live && epoch === lifecycle.current && allowed()) setState(value); })
      .catch(cause => { if (live && epoch === lifecycle.current && allowed()) setError(cause instanceof Error ? cause.message : "Kunne ikkje hente agentval."); });
    return () => { live = false; lifecycle.current++; };
  }, [api, channelId, reload]);
  const select = async (agentId: string, enabled: boolean) => {
    const epoch = lifecycle.current, allowed = guard.current;
    const current = () => epoch === lifecycle.current && allowed();
    if (!state || mutationPending.current || !current()) return;
    mutationPending.current = true;
    setPending(true); setError("");
    try { const saved = await api.selectChannel(channelId, agentId, enabled, state.accessRevision); if (current()) setState(saved); }
    catch (cause) { if (current()) setError(cause instanceof Error ? cause.message : "Kunne ikkje lagre agentval. Hent vala på nytt før du prøver igjen."); }
    finally { if (current()) { mutationPending.current = false; setPending(false); } }
  };
  return <div className="sp-agent-channel-access">
    <p>{privateChannel
      ? "Private kanalar har ingen agentar før du vel dei her. Ein vald agent får lese dei siste 20 minutta i denne kanalen eller tråden når han blir utløyst."
      : "Vel kva kretsagentar som får svare her. Kretsen styrer oppsettet; du styrer tilgangen til denne kanalen."}</p>
    {state ? <div className="sp-channel-agent-list">
      {!state.selectionAvailable && <><Status>Kanalval blir aktivert når utrullinga er ferdig.</Status><Button onClick={() => setReload(value => value + 1)}>Hent vala på nytt</Button></>}
      {state.agents.length === 0 && <Status>Ingen agentar i kretsen enno.</Status>}
      {state.agents.filter(agent => !agentId || agent.agentId === agentId).map(agent => <div key={agent.agentId} className="sp-channel-agent-row" data-linked={onOpenAgent ? "true" : undefined}>
        <label className="sp-channel-agent-choice">
          <input type="checkbox" aria-label={agent.displayName} checked={agent.enabled} disabled={pending || !!error || !state.selectionAvailable} onChange={event => void select(agent.agentId, event.currentTarget.checked)} />
          {!onOpenAgent && <span>{agent.displayName}</span>}
        </label>
        {onOpenAgent && <Button variant="quiet" className="sp-channel-agent-open" aria-label={`Opne agentoppsett for ${agent.displayName}`}
          onClick={() => { if (guard.current()) onOpenAgent(agent.agentId); }}>{agent.displayName}<span aria-hidden="true">›</span></Button>}
        {!agent.agentEnabled && <small>Avslått i kretsen – må også slåast på under Agentar i kretsmenyen.</small>}
      </div>)}
    </div> : !error && <Status>Hentar agentval …</Status>}
    {pending && <Status>Lagrar agentval …</Status>}
    {error && <><Status tone="error">{error}</Status><Button disabled={pending} onClick={() => setReload(value => value + 1)}>Hent vala på nytt</Button></>}
    <p className="sp-help">Når du slår av tilgang, blir ventande svar stoppa. Svar som allereie er sende, blir ståande.</p>
  </div>;
}
