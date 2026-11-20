import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

type TcpResult = 'not_needed' | 'need_address' | 'choose_receiver' | 'invalid_address' |
  'reachable' | 'refused' | 'timed_out' | 'unreachable';

type Report = {
  version: string;
  role: 'primary' | 'secondary';
  phase: string;
  confirmation: 'none' | 'required' | 'waiting_for_peer';
  receiver_discovery: boolean | null;
  discovery: {
    available: boolean;
    interfaces: { kind: string; replies: number | null }[];
    receiver_count: number;
  } | null;
  tcp: TcpResult;
};

const tcpMessages: Record<TcpResult, string> = {
  not_needed: '',
  need_address: 'TCP was not tested. Enter the receiver’s IPv4 address to check it directly.',
  choose_receiver: 'TCP was not tested. Enter the IPv4 address of the receiver you want to check.',
  invalid_address: 'Enter a valid receiver IPv4 address.',
  reachable: 'Receiver TCP port 46387 is reachable. Pairing must still be completed in EdgeBridge.',
  refused: 'The other PC refused TCP port 46387. Make sure its receiver is running.',
  timed_out: 'TCP port 46387 timed out. Check the LAN connection and the receiver’s firewall.',
  unreachable: 'Could not reach TCP port 46387. Check the receiver address and network.',
};

function reportLines(report: Report): string[] {
  const lines = [`Version ${report.version}`, `Role ${report.role}`, `Session ${report.phase}`];
  if (report.confirmation === 'required') lines.push('Compare the number on both PCs and confirm pairing.');
  if (report.confirmation === 'waiting_for_peer') lines.push('Waiting for the other PC to confirm pairing.');
  if (report.phase === 'connected') lines.push('EdgeBridge already has an authenticated connection.');
  if (report.receiver_discovery === true) lines.push('The receiver is listening and LAN discovery is enabled.');
  if (report.receiver_discovery === false) lines.push('The receiver is listening. Use manual IPv4 because LAN discovery is unavailable.');
  if (report.role === 'secondary' && report.phase === 'idle') lines.push('Start the receiver on this PC.');
  if (report.discovery) {
    const { available, interfaces, receiver_count } = report.discovery;
    if (!available) lines.push('Windows could not list the LAN interfaces.');
    else if (!interfaces.length) lines.push('There are no active IPv4 interfaces that support broadcast discovery.');
    else {
      if (interfaces.every(({ replies }) => replies === null)) {
        lines.push('Discovery failed on every tested interface.');
      } else {
        lines.push(receiver_count === 0 ? 'No receiver replied to discovery.' : `Receivers found: ${receiver_count}`);
      }
      interfaces.forEach(({ kind, replies }) => lines.push(
        `${kind}: ${replies === null ? 'scan failed' : `${replies} replies`}`,
      ));
    }
  }
  if (tcpMessages[report.tcp]) lines.push(tcpMessages[report.tcp]);
  return lines;
}

export function ConnectionDiagnostics({ primary, connected, disabled }: {
  primary: boolean;
  connected: boolean;
  disabled: boolean;
}) {
  const [report, setReport] = useState<Report | null>(null);
  const [address, setAddress] = useState('');
  const [running, setRunning] = useState(false);
  const [message, setMessage] = useState('');
  const lines = report ? reportLines(report) : [];

  async function diagnose() {
    setRunning(true);
    setMessage('');
    setReport(null);
    try {
      setReport(await invoke<Report>('diagnose_connection', { primary, address }));
    } catch {
      setMessage('Could not run connection diagnostics.');
    } finally {
      setRunning(false);
    }
  }

  async function copy() {
    setMessage('');
    try {
      await invoke('copy_connection_diagnostic', { report: ['EdgeBridge connection diagnostic', ...lines].join('\n') });
      setMessage('Diagnostic copied.');
    } catch (err) {
      setMessage(String(err));
    }
  }

  return <section className="mt-4" aria-label="Connection diagnostics">
    <button type="button" className="secondary" disabled={disabled || running} onClick={() => void diagnose()}>
      {running ? 'Diagnosing…' : report ? 'Run diagnostic again' : 'Diagnose connection'}
    </button>
    {report && <div className="panel mt-3">
      <ul className="space-y-1 text-sm" aria-label="Diagnostic results">
        {lines.map((line, index) => <li key={index}>{line}</li>)}
      </ul>
      {primary && !connected && report.confirmation === 'none' && <label className="field mt-3">
        Receiver IPv4 (optional)
        <input value={address} onChange={(event) => setAddress(event.target.value)} placeholder="192.168.1.42"
          inputMode="decimal" autoComplete="off" spellCheck={false} />
      </label>}
      <button type="button" className="secondary mt-3" onClick={() => void copy()}>Copy diagnostic</button>
    </div>}
    {message && <p className="hint" role="status">{message}</p>}
  </section>;
}
