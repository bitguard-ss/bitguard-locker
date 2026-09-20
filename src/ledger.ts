import TransportWebHID from "@ledgerhq/hw-transport-webhid";
import Solana from "@ledgerhq/hw-app-solana";
import { PublicKey } from "@solana/web3.js";

export type PathStyle = "ledger-live" | "with-change";

/**
 * Solana derivation paths, hardened per SLIP-0010 (required for ed25519).
 * "ledger-live" (m/44'/501'/{index}') is what Ledger Live itself uses for
 * account 0, 1, 2, ... "with-change" (m/44'/501'/{index}'/0') is the
 * convention some other wallets use when importing a Ledger account.
 * If you have "many wallets" on one Ledger, they're most likely just
 * different indexes under one of these two conventions — the scanner in
 * App.tsx checks both.
 */
export function accountPath(index: number, style: PathStyle): string {
  return style === "ledger-live" ? `44'/501'/${index}'` : `44'/501'/${index}'/0'`;
}

export async function connectLedger(): Promise<{ transport: any; solana: Solana }> {
  if (!(navigator as any).hid) {
    throw new Error(
      "This browser doesn't support WebHID. Use Chrome or Edge on desktop."
    );
  }
  const transport = await TransportWebHID.create();
  const solana = new Solana(transport as any);
  return { transport, solana };
}

export async function deriveAddress(solana: Solana, path: string): Promise<PublicKey> {
  const { address } = await solana.getAddress(path);
  return new PublicKey(address);
}
