import { useEffect, useMemo, useRef, useState } from "react";
import {
  Connection,
  PublicKey,
  Transaction,
} from "@solana/web3.js";
import {
  getAssociatedTokenAddress,
  getAccount,
  getMint,
  createAssociatedTokenAccountInstruction,
  createTransferCheckedInstruction,
  TokenAccountNotFoundError,
} from "@solana/spl-token";
import type Solana from "@ledgerhq/hw-app-solana";

import { SOURCE_WALLET, MINT_ADDRESS, DESTINATION_WALLET, RPC_ENDPOINT } from "./config";
import { accountPath, connectLedger, deriveAddress, type PathStyle } from "./ledger";

function tryParseAddress(value: string): PublicKey | null {
  try {
    return new PublicKey(value.trim());
  } catch {
    return null;
  }
}

type Status =
  | { kind: "idle" }
  | { kind: "loading"; message: string }
  | { kind: "error"; message: string }
  | { kind: "success"; signature: string };

type ScanRow = {
  index: number;
  style: PathStyle;
  path: string;
  address: string;
  tokenBalance: string | null; // null = not checked / no valid mint yet
};

const connection = new Connection(RPC_ENDPOINT, "confirmed");

export default function App() {
  // --- Ledger connection state ---
  const solanaAppRef = useRef<Solana | null>(null);
  const transportRef = useRef<any>(null);
  const [ledgerConnected, setLedgerConnected] = useState(false);
  const [connectStatus, setConnectStatus] = useState<string | null>(null);

  // --- account selection ---
  const [accountIndex, setAccountIndex] = useState(0);
  const [pathStyle, setPathStyle] = useState<PathStyle>("ledger-live");
  const [derivedAddress, setDerivedAddress] = useState<PublicKey | null>(null);
  const [deriveError, setDeriveError] = useState<string | null>(null);

  const [scanning, setScanning] = useState(false);
  const [scanRows, setScanRows] = useState<ScanRow[]>([]);

  // --- editable transfer fields ---
  const [expectedSource, setExpectedSource] = useState(SOURCE_WALLET);
  const [mintAddress, setMintAddress] = useState(MINT_ADDRESS);
  const [destinationAddress, setDestinationAddress] = useState(DESTINATION_WALLET);

  const [decimals, setDecimals] = useState<number | null>(null);
  const [balanceRaw, setBalanceRaw] = useState<bigint | null>(null);
  const [amount, setAmount] = useState("");
  const [status, setStatus] = useState<Status>({ kind: "idle" });

  const trimmedExpectedSource = expectedSource.trim();
  const expectedSourceIsValidAddress =
    trimmedExpectedSource.length === 0 || tryParseAddress(trimmedExpectedSource) !== null;

  const mint = useMemo(() => tryParseAddress(mintAddress), [mintAddress]);
  const destination = useMemo(() => tryParseAddress(destinationAddress), [destinationAddress]);
  const mintIsValid = mintAddress.trim().length === 0 || mint !== null;
  const destinationIsValid = destinationAddress.trim().length === 0 || destination !== null;

  const addressMatches =
    derivedAddress !== null &&
    expectedSourceIsValidAddress &&
    trimmedExpectedSource.length > 0 &&
    derivedAddress.toBase58() === trimmedExpectedSource;

  const currentPath = accountPath(accountIndex, pathStyle);

  // Connect to the Ledger over WebHID (direct USB), no browser wallet involved.
  async function handleConnect() {
    setConnectStatus("Requesting device (pick your Ledger in the browser prompt)...");
    setDeriveError(null);
    try {
      const { transport, solana } = await connectLedger();
      transportRef.current = transport;
      solanaAppRef.current = solana;
      setLedgerConnected(true);
      setConnectStatus(null);
      transport.on?.("disconnect", () => {
        setLedgerConnected(false);
        solanaAppRef.current = null;
        setDerivedAddress(null);
      });
    } catch (e: any) {
      setConnectStatus(null);
      setDeriveError(e.message ?? String(e));
    }
  }

  function handleDisconnect() {
    transportRef.current?.close?.();
    transportRef.current = null;
    solanaAppRef.current = null;
    setLedgerConnected(false);
    setDerivedAddress(null);
    setScanRows([]);
  }

  // Re-derive the address whenever the connected device, index, or path style changes.
  useEffect(() => {
    setDerivedAddress(null);
    setDeriveError(null);
    if (!ledgerConnected || !solanaAppRef.current) return;

    let cancelled = false;
    (async () => {
      try {
        const pk = await deriveAddress(solanaAppRef.current!, currentPath);
        if (!cancelled) setDerivedAddress(pk);
      } catch (e: any) {
        if (!cancelled) setDeriveError(e.message ?? String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [ledgerConnected, accountIndex, pathStyle, currentPath]);

  // Scan indexes 0-9 under both path conventions, and check each one's balance
  // for the currently-entered mint (if valid) — this is the fastest way to
  // find which of several Ledger accounts actually holds the token.
  async function handleScan() {
    if (!solanaAppRef.current) return;
    setScanning(true);
    setScanRows([]);
    const rows: ScanRow[] = [];
    const styles: PathStyle[] = ["ledger-live", "with-change"];
    try {
      for (const style of styles) {
        for (let i = 0; i < 10; i++) {
          const path = accountPath(i, style);
          const pk = await deriveAddress(solanaAppRef.current!, path);
          let tokenBalance: string | null = null;
          if (mint) {
            try {
              const ata = await getAssociatedTokenAddress(mint, pk);
              const account = await getAccount(connection, ata);
              const mintInfo = await getMint(connection, mint);
              tokenBalance = String(Number(account.amount) / 10 ** mintInfo.decimals);
            } catch (e) {
              tokenBalance = e instanceof TokenAccountNotFoundError ? "0" : "error";
            }
          }
          rows.push({ index: i, style, path, address: pk.toBase58(), tokenBalance });
          setScanRows([...rows]);
        }
      }
    } catch (e: any) {
      setDeriveError(e.message ?? String(e));
    } finally {
      setScanning(false);
    }
  }

  function useScanRow(row: ScanRow) {
    setAccountIndex(row.index);
    setPathStyle(row.style);
    setExpectedSource(row.address);
  }

  // Pull mint decimals + the derived address's token balance whenever it changes.
  useEffect(() => {
    setBalanceRaw(null);
    setDecimals(null);
    if (!addressMatches || !mint || !derivedAddress) return;

    let cancelled = false;
    (async () => {
      try {
        const mintInfo = await getMint(connection, mint);
        if (cancelled) return;
        setDecimals(mintInfo.decimals);

        const ata = await getAssociatedTokenAddress(mint, derivedAddress);
        try {
          const account = await getAccount(connection, ata);
          if (!cancelled) setBalanceRaw(account.amount);
        } catch (e) {
          if (e instanceof TokenAccountNotFoundError) {
            if (!cancelled) setBalanceRaw(0n);
          } else {
            throw e;
          }
        }
      } catch (e: any) {
        if (!cancelled) {
          setStatus({ kind: "error", message: `Couldn't load token info: ${e.message ?? e}` });
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [addressMatches, mint, derivedAddress]);

  const humanBalance = useMemo(() => {
    if (balanceRaw === null || decimals === null) return null;
    return Number(balanceRaw) / 10 ** decimals;
  }, [balanceRaw, decimals]);

  function setMax() {
    if (humanBalance !== null) setAmount(String(humanBalance));
  }

  async function handleSend() {
    const solana = solanaAppRef.current;
    if (!solana || !derivedAddress || decimals === null || balanceRaw === null) return;
    if (!mint || !destination) {
      setStatus({ kind: "error", message: "Fix the token mint / destination address first." });
      return;
    }

    const parsed = Number(amount);
    if (!Number.isFinite(parsed) || parsed <= 0) {
      setStatus({ kind: "error", message: "Enter a valid amount greater than 0." });
      return;
    }

    const rawAmount = BigInt(Math.round(parsed * 10 ** decimals));
    if (rawAmount > balanceRaw) {
      setStatus({ kind: "error", message: "Amount exceeds the wallet's token balance." });
      return;
    }

    try {
      setStatus({ kind: "loading", message: "Building transaction..." });

      const sourceAta = await getAssociatedTokenAddress(mint, derivedAddress);
      const destAta = await getAssociatedTokenAddress(mint, destination);

      const tx = new Transaction();

      const destAtaInfo = await connection.getAccountInfo(destAta);
      if (!destAtaInfo) {
        tx.add(
          createAssociatedTokenAccountInstruction(derivedAddress, destAta, destination, mint)
        );
      }

      tx.add(
        createTransferCheckedInstruction(
          sourceAta,
          mint,
          destAta,
          derivedAddress,
          rawAmount,
          decimals
        )
      );

      const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash("confirmed");
      tx.recentBlockhash = blockhash;
      tx.feePayer = derivedAddress;

      setStatus({ kind: "loading", message: "Confirm the transaction on your Ledger screen..." });
      const messageBytes = tx.serializeMessage();
      const { signature } = await solana.signTransaction(currentPath, messageBytes);
      tx.addSignature(derivedAddress, signature as Buffer);

      setStatus({ kind: "loading", message: "Sending transaction..." });
      const sig = await connection.sendRawTransaction(tx.serialize());

      setStatus({ kind: "loading", message: "Waiting for confirmation..." });
      await connection.confirmTransaction({ signature: sig, blockhash, lastValidBlockHeight }, "confirmed");

      setStatus({ kind: "success", signature: sig });
      setAmount("");
      const account = await getAccount(connection, sourceAta);
      setBalanceRaw(account.amount);
    } catch (e: any) {
      setStatus({ kind: "error", message: e.message ?? String(e) });
    }
  }

  return (
    <div className="page">
      <header className="header">
        <h1>Token Transfer</h1>
        <p className="subtitle">Ledger connects directly over USB &mdash; no Phantom, no browser extension.</p>
      </header>

      <section className="panel">
        <div className="row">
          <span className="label">Expected source</span>
          <input
            className="mono address-input"
            type="text"
            value={expectedSource}
            onChange={(e) => setExpectedSource(e.target.value)}
            spellCheck={false}
          />
        </div>
        {!expectedSourceIsValidAddress && (
          <p className="warning">That doesn't look like a valid Solana address.</p>
        )}
        <div className="row">
          <span className="label">Token mint</span>
          <input
            className="mono address-input"
            type="text"
            value={mintAddress}
            onChange={(e) => setMintAddress(e.target.value)}
            spellCheck={false}
          />
        </div>
        {!mintIsValid && <p className="warning">That doesn't look like a valid Solana address.</p>}
        <div className="row">
          <span className="label">Destination</span>
          <input
            className="mono address-input"
            type="text"
            value={destinationAddress}
            onChange={(e) => setDestinationAddress(e.target.value)}
            spellCheck={false}
          />
        </div>
        {!destinationIsValid && <p className="warning">That doesn't look like a valid Solana address.</p>}
      </section>

      <section className="panel">
        {!ledgerConnected ? (
          <button type="button" className="primary" onClick={handleConnect}>
            Connect Ledger (USB)
          </button>
        ) : (
          <div className="ledger-controls">
            <div className="row">
              <span className="label">Device</span>
              <span>Connected via USB</span>
              <button type="button" className="ghost" onClick={handleDisconnect}>
                Disconnect
              </button>
            </div>

            <div className="row">
              <span className="label">Account index</span>
              <input
                type="number"
                min="0"
                value={accountIndex}
                onChange={(e) => setAccountIndex(Math.max(0, Number(e.target.value) || 0))}
                style={{ width: "80px" }}
              />
              <select value={pathStyle} onChange={(e) => setPathStyle(e.target.value as PathStyle)}>
                <option value="ledger-live">Ledger Live path (44'/501'/i')</option>
                <option value="with-change">With change level (44'/501'/i'/0')</option>
              </select>
            </div>

            <div className="row">
              <span className="label">Path</span>
              <span className="mono">{currentPath}</span>
            </div>

            <div className="row">
              <span className="label">Derived address</span>
              <span className="mono">{derivedAddress ? derivedAddress.toBase58() : "..."}</span>
            </div>

            {deriveError && <p className="error">{deriveError}</p>}

            <button type="button" className="ghost" onClick={handleScan} disabled={scanning}>
              {scanning ? "Scanning..." : "Scan accounts 0-9 (both path styles)"}
            </button>

            {scanRows.length > 0 && (
              <div className="scan-results">
                {scanRows.map((row) => {
                  const isMatch = row.address === trimmedExpectedSource;
                  return (
                    <div key={`${row.style}-${row.index}`} className={`scan-row${isMatch ? " scan-row-match" : ""}`}>
                      <span className="mono">{row.address}</span>
                      <span className="scan-meta">{row.path}</span>
                      {row.tokenBalance !== null && (
                        <span className="scan-meta">balance: {row.tokenBalance}</span>
                      )}
                      <button type="button" className="ghost" onClick={() => useScanRow(row)}>
                        Use this
                      </button>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        )}

        {connectStatus && <p className="status">{connectStatus}</p>}

        {ledgerConnected && derivedAddress && !addressMatches && (
          <p className="warning">
            Derived address doesn't match the expected source above. Sending is
            disabled. Adjust the account index/path, use a row from the scan
            results, or update the Expected source field.
          </p>
        )}

        {ledgerConnected && addressMatches && (
          <div className="transfer-form">
            <div className="row">
              <span className="label">Balance</span>
              <span className="mono">
                {humanBalance === null ? "loading..." : humanBalance}
              </span>
            </div>

            <div className="amount-row">
              <input
                type="number"
                min="0"
                step="any"
                placeholder="Amount to send"
                value={amount}
                onChange={(e) => setAmount(e.target.value)}
              />
              <button type="button" className="ghost" onClick={setMax} disabled={humanBalance === null}>
                Max
              </button>
            </div>

            <button
              type="button"
              className="primary"
              onClick={handleSend}
              disabled={status.kind === "loading" || !amount || !mint || !destination}
            >
              {status.kind === "loading" ? "Working..." : "Send transfer"}
            </button>
          </div>
        )}

        {status.kind === "loading" && <p className="status">{status.message}</p>}
        {status.kind === "error" && <p className="error">{status.message}</p>}
        {status.kind === "success" && (
          <p className="success">
            Sent. View on{" "}
            <a
              href={`https://explorer.solana.com/tx/${status.signature}`}
              target="_blank"
              rel="noreferrer"
            >
              Solana Explorer
            </a>
            .
          </p>
        )}
      </section>
    </div>
  );
}
