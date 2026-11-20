import { useEffect, useState } from 'react';
import type { FormEvent } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import './styles.css';

type Role = 'primary' | 'secondary';
type Edge = 'left' | 'right' | 'top' | 'bottom';
type Phase = 'idle' | 'starting' | 'waiting' | 'connected' | 'reconnecting' | 'stopping' | 'error';
type Advertisement = { name: string; pin: number[] };
type Receiver = { address: string; advertisement: Advertisement | null };
type Approval = { id: number; name: string; comparison: string; accepted: boolean };
type Status = {
  phase: Phase;
  role: Role | null;
  code: string | null;
  address: string | null;
  peer: string | null;
  latency_ms: number | null;
  forwarding: boolean;
  clipboard_active: boolean;
  approval: Approval | null;
  message: string | null;
};

const labels: Record<Phase, string> = {
  idle: 'Disconnected',
  starting: 'Starting',
  waiting: 'Ready to pair',
  connected: 'Connected',
  reconnecting: 'Reconnecting',
  stopping: 'Stopping',
  error: 'Connection error',
};
const edges: Edge[] = ['left', 'right', 'top', 'bottom'];
const opposite: Record<Edge, Edge> = { left: 'right', right: 'left', top: 'bottom', bottom: 'top' };
const positionLabels: Record<Edge, string> = { left: 'Left', right: 'Right', top: 'Above', bottom: 'Below' };

function savedEdge(key: string, fallback: Edge): Edge {
  const stored = localStorage.getItem(key);
  return edges.find((edge) => edge === stored) ?? fallback;
}

function LayoutPreview({ from, to }: { from: Edge; to: Edge }) {
  const vertical = from === 'top' || from === 'bottom';
  const secondaryFirst = from === 'left' || from === 'top';
  const primary = <span className="screen" data-edge={from}>Primary</span>;
  const secondary = <span className="screen" data-edge={to}>Secondary</span>;
  return <div className="preview" data-vertical={vertical} aria-hidden="true">
    {secondaryFirst ? secondary : primary}
    <span className="arrows">{vertical ? '↕' : '⇄'}</span>
    {secondaryFirst ? primary : secondary}
  </div>;
}

function EdgeField({ label, value, change, disabled }: {
  label: string;
  value: Edge;
  change: (edge: Edge) => void;
  disabled: boolean;
}) {
  return <label className="field">
    {label}
    <select value={value} disabled={disabled} onChange={(event) => change(event.target.value as Edge)}>
      {edges.map((edge) => <option key={edge} value={edge}>{edge}</option>)}
    </select>
  </label>;
}

function App() {
  const [status, setStatus] = useState<Status | null>(null);
  const [shareClipboard, setShareClipboard] = useState(false);
  const [role, setRole] = useState<Role>(() => localStorage.getItem('role') === 'secondary' ? 'secondary' : 'primary');
  const [address, setAddress] = useState('');
  const [code, setCode] = useState('');
  const [from, setFrom] = useState<Edge>(() => savedEdge('primary-edge', 'left'));
  const [to, setTo] = useState<Edge>(() => savedEdge('secondary-edge', 'right'));
  const [discovered, setDiscovered] = useState<Receiver[]>([]);
  const [scanMessage, setScanMessage] = useState('');
  const [cursorMessage, setCursorMessage] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [update, setUpdate] = useState<string | null>(null);
  const [updateMessage, setUpdateMessage] = useState('');
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [installing, setInstalling] = useState(false);
  const hasStatus = status !== null;
  const running = !!status?.role;
  const selected = status?.role ?? role;
  const disabled = running || busy || installing;
  const approval = status?.approval;

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const next = await invoke<Status>('status');
        if (alive) setStatus(next);
      } catch {
        if (alive) setError('Open EdgeBridge in the Windows desktop app.');
      } finally {
        if (alive) timer = setTimeout(poll, 250);
      }
    }
    void poll();
    return () => { alive = false; clearTimeout(timer); };
  }, []);

  useEffect(() => {
    localStorage.setItem('primary-edge', from);
    localStorage.setItem('secondary-edge', to);
  }, [from, to]);

  useEffect(() => { localStorage.setItem('role', role); }, [role]);

  useEffect(() => {
    if (running || !hasStatus) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function detect() {
      try {
        const results = await invoke<Receiver[]>('discover');
        if (active) {
          setDiscovered(results);
          if (results.length === 1) setAddress((current) => current || results[0].address);
        }
      } catch {
        // The manual search reports errors. Background discovery is optional.
      } finally {
        if (active) timer = setTimeout(detect, 5000);
      }
    }
    void detect();
    return () => { active = false; clearTimeout(timer); };
  }, [hasStatus, running]);

  async function checkUpdates(manual: boolean) {
    setCheckingUpdate(true);
    setUpdateMessage('');
    try {
      const version = await invoke<string | null>('check_update');
      setUpdate(version);
      if (manual && !version) setUpdateMessage('EdgeBridge is up to date.');
    } catch {
      if (manual) setUpdateMessage('Could not check for updates. Try again later.');
    } finally {
      setCheckingUpdate(false);
    }
  }

  useEffect(() => {
    void checkUpdates(false);
    const timer = setInterval(() => void checkUpdates(false), 6 * 60 * 60 * 1000);
    return () => clearInterval(timer);
  }, []);

  async function installUpdate() {
    if (!update) return;
    setInstalling(true);
    setUpdateMessage('');
    try {
      await invoke('install_update', { version: update });
    } catch (err) {
      setUpdateMessage(String(err));
      setInstalling(false);
    }
  }

  async function action(command: string, args?: Record<string, unknown>) {
    setError('');
    setBusy(true);
    try {
      await invoke(command, args);
      setStatus(await invoke<Status>('status'));
      if (command === 'connect' || command === 'stop') setCode('');
      return true;
    } catch (err) {
      setError(String(err));
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function scan() {
    setScanning(true);
    setScanMessage('');
    try {
      const results = await invoke<Receiver[]>('discover');
      setDiscovered(results);
      if (results.length === 1) setAddress(results[0].address);
      if (!results.length) setScanMessage('No receiver replied. Check that both PCs are on the same LAN and the receiver is ready, or pair manually by IPv4.');
    } catch {
      setScanMessage('EdgeBridge could not search the LAN interfaces. Enter the receiver’s IPv4 address below.');
    } finally {
      setScanning(false);
    }
  }

  function connect(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void action('connect', { address, code, from, to, shareClipboard });
  }

  function setPosition(edge: Edge) {
    setFrom(edge);
    setTo(opposite[edge]);
  }

  async function swapRole() {
    if (running && !await action('stop')) return;
    setRole(selected === 'primary' ? 'secondary' : 'primary');
    setFrom(to);
    setTo(from);
  }

  function requestPairing(receiver: Receiver) {
    if (!receiver.advertisement) return;
    setRole('primary');
    void action('request_pairing', {
      address: receiver.address, advertisement: receiver.advertisement, from, to, shareClipboard,
    });
  }

  async function restoreCursor() {
    setCursorMessage('');
    try {
      await invoke('restore_cursor');
      setCursorMessage('Windows cursor scheme reloaded.');
    } catch (err) {
      setCursorMessage(String(err));
    }
  }

  return <main className="mx-auto max-w-2xl px-5 py-8 sm:px-9 sm:py-10">
    <header className="mb-7 flex items-center justify-between gap-3">
      <div className="flex items-center gap-3">
        <span className="brand" aria-hidden="true"><i /><i /></span>
        <h1 className="text-2xl font-semibold tracking-tight">EdgeBridge KVM</h1>
      </div>
      <div className="status" data-connected={status?.phase === 'connected'} role="status">
        <span aria-hidden="true" />{approval ? 'Confirm pairing' : labels[status?.phase ?? 'idle']}
      </div>
    </header>

    <div className="roles mb-5" aria-label="This computer’s role">
      <button type="button" disabled={disabled} aria-pressed={selected === 'primary'} onClick={() => setRole('primary')}>Primary PC</button>
      <button type="button" disabled={disabled} aria-pressed={selected === 'secondary'} onClick={() => setRole('secondary')}>Secondary PC</button>
    </div>

    <label className="mb-4 flex items-center gap-2 text-sm">
      <input type="checkbox" className="clipboard-toggle" checked={shareClipboard} disabled={disabled}
        onChange={(event) => setShareClipboard(event.target.checked)} />
      Share clipboard on both PCs
    </label>
    {approval && <section className="panel mb-4" aria-labelledby="approval-title">
      <h2 id="approval-title">Pair with {approval.name}?</h2>
      <p className="description">Compare the number on both PCs. Accept on both only if it matches.</p>
      <p className="comparison" aria-label={`Pairing number ${approval.comparison.split('').join(' ')}`}>
        {approval.comparison.slice(0, 4)} {approval.comparison.slice(4)}
      </p>
      {approval.accepted ? <p className="hint" role="status">Waiting for the other PC to accept.</p> : <div className="actions">
        <button className="primary" type="button" disabled={busy} onClick={() =>
          void action('decide_pairing', { id: approval.id, accept: true })}>Numbers match</button>
        <button className="secondary" type="button" disabled={busy} onClick={() =>
          void action('decide_pairing', { id: approval.id, accept: false })}>Decline</button>
      </div>}
    </section>}
    {selected === 'secondary' && !running && discovered.some((pc) => pc.advertisement) &&
      <p className="notice" role="status">
        {discovered.filter((pc) => pc.advertisement).map((pc) => pc.advertisement!.name).join(', ')} available to pair.
        <button className="secondary ml-2" type="button" onClick={() => setRole('primary')}>Use this PC as primary</button>
      </p>}
    {selected === 'primary' ? <form className="panel" onSubmit={connect}>
      <h2>Connect to the secondary PC</h2>
      <p className="description">Choose the other PC once its receiver is ready.</p>
      {!running && <div className="actions">
        {discovered.map((pc) => pc.advertisement && <button className="primary" key={pc.address}
          type="button" disabled={disabled} onClick={() => requestPairing(pc)}>
          Pair with {pc.advertisement.name} <small>({pc.address})</small>
        </button>)}
        <button className="secondary" type="button" disabled={disabled || scanning} onClick={() => void scan()}>
          {scanning ? 'Scanning…' : 'Find on Wi-Fi'}
        </button>
      </div>}
      {scanMessage && <p className="hint" role="status">{scanMessage}</p>}
      <LayoutPreview from={from} to={to} />
      <fieldset className="mb-3" disabled={disabled}>
        <legend className="field">Where is the secondary PC?</legend>
        <div className="position-options">
          {edges.map((edge) => <button key={edge} className="secondary" type="button"
            aria-pressed={from === edge && to === opposite[edge]} onClick={() => setPosition(edge)}>
            {positionLabels[edge]}
          </button>)}
        </div>
      </fieldset>
      {!running && <details>
        <summary>Manual pairing and custom edges</summary>
        <div className="mt-3 grid grid-cols-2 gap-3">
          <EdgeField label="Primary exit edge" value={from} change={setFrom} disabled={disabled} />
          <EdgeField label="Secondary entry and return edge" value={to} change={setTo} disabled={disabled} />
        </div>
      <label className="field mt-4" htmlFor="address">Secondary IPv4 address</label>
      <div className="flex flex-wrap gap-2">
        <input className="min-w-28 flex-1" id="address" value={address} required disabled={disabled} placeholder="192.168.1.42"
          onChange={(event) => setAddress(event.target.value)} autoComplete="off" spellCheck={false} />
      </div>
      {discovered.length > 1 && <ul className="flex flex-wrap gap-2 py-2" aria-label="Discovered receivers">
        {discovered.map(({ address }) => <li key={address}><button className="secondary" type="button" onClick={() => setAddress(address)}>{address}</button></li>)}
      </ul>}
      <label className="field mt-4">
        Pairing code
        <input value={code} type="password" required disabled={busy} placeholder="EB1-…" spellCheck={false}
          autoComplete="off" onChange={(event) => setCode(event.target.value)} />
      </label>
        <button className="primary mt-3" disabled={disabled} type="submit">Connect with code</button>
      </details>}
      <div className="actions">
        {running && <button className="secondary" type="button" disabled={busy} onClick={() => void action('stop')}>Disconnect</button>}
        <span className="hint">Emergency return <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>F12</kbd></span>
      </div>
    </form> : <section className="panel" aria-labelledby="receiver-title">
      <h2 id="receiver-title">Allow control of this PC</h2>
      <p className="description">Start the receiver to appear on the other PC. Confirm the request here when it arrives.</p>
      {status?.address && <div className="fact"><span>Suggested IPv4 address</span><code>{status.address}</code></div>}
      {status?.code && <details className="mt-3"><summary>Show manual pairing code</summary>
        <label className="field mt-3">
          Receiver pairing code
          <input readOnly spellCheck={false} value={status.code} onFocus={(event) => event.currentTarget.select()} />
          <span className="hint">Copy or share the code privately. Stopping the receiver revokes it.</span>
        </label>
      </details>}
      <div className="actions">
        {running
          ? <button className="secondary" type="button" disabled={busy} onClick={() => void action('stop')}>Stop receiver</button>
          : <button className="primary" type="button" disabled={busy} onClick={() => void action('listen', { shareClipboard })}>Start receiver</button>}
      </div>
    </section>}
    <div className="mt-4 flex flex-wrap items-center gap-2">
      <button className="secondary" type="button" disabled={busy || installing || !!approval} onClick={() => void swapRole()}>
        {selected === 'primary' ? 'Make this PC secondary' : 'Make this PC primary'}
      </button>
      <span className="hint">Switch roles on both PCs and reconnect. {running && 'This ends the current session.'}</span>
    </div>

    {running && <section className="mt-4 grid gap-2 rounded-2xl border border-slate-200 bg-white px-4 py-3 text-sm sm:grid-cols-3"
      aria-label="Session status">
      <div><span className="metric">Control</span><strong>{status?.forwarding ? 'Secondary PC' : 'Local'}</strong></div>
      <div><span className="metric">Peer</span><strong>{status?.peer ?? '—'}</strong></div>
      <div><span className="metric">LAN round trip</span><strong>{status?.latency_ms == null ? '—' : status.latency_ms + ' ms'}</strong></div>
    </section>}
    {running && shareClipboard && status?.phase === 'connected' && <p className="hint" role="status">
      {status.clipboard_active ? 'Clipboard sharing active' : 'Clipboard sharing unavailable on this connection'}
    </p>}
    <div className="mt-4 flex flex-wrap items-center gap-2">
      <button type="button" className="secondary" disabled={installing} onClick={() => void restoreCursor()}>Restore cursor</button>
      {cursorMessage && <span className="hint" role="status">{cursorMessage}</span>}
    </div>
    <section className="mt-5 flex flex-wrap items-center gap-3" aria-label="Software updates">
      {update ? <>
        <button type="button" className="secondary" disabled={!hasStatus || disabled}
          onClick={() => void installUpdate()}>
          {installing ? 'Installing…' : `Install update ${update}`}
        </button>
        {running && <span className="hint">Stop the session to install.</span>}
      </> : <button type="button" className="secondary" disabled={checkingUpdate}
        onClick={() => void checkUpdates(true)}>
        {checkingUpdate ? 'Checking for updates…' : 'Check for updates'}
      </button>}
      {updateMessage && <span className="hint" role="status">{updateMessage}</span>}
    </section>
    {(error || status?.message) && <p className="notice" role={error ? 'alert' : 'status'}>{error || status?.message}</p>}
  </main>;
}

createRoot(document.getElementById('root')!).render(<App />);
