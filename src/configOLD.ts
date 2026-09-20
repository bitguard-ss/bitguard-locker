/**
 * Everything this app does is pinned to these constants.
 *
 * SOURCE_WALLET is the only value you should normally need to change —
 * it's the address the app expects your Ledger to present. If you plug in
 * a different Ledger account, the app will warn you and refuse to send
 * rather than silently moving funds from the wrong account.
 *
 * MINT_ADDRESS and DESTINATION_WALLET are fixed for this transfer. Edit
 * them here (not in App.tsx) if you ever need a different token or a
 * different recipient.
 */

// The Ledger-held wallet this app expects to be the sender.
export const SOURCE_WALLET = "2vcAeWjy5XKdP1HXYkFK5uWy9u1UCXpYQvtyybshiDNy";

// The SPL token mint being transferred.
export const MINT_ADDRESS = "6ENavE5QXLFrJLBLdgExqPWPRgMRfNJ13kEn8oBmCG8N";

// The Phantom wallet receiving the tokens.
export const DESTINATION_WALLET = "6EbE8YjwaJayTAXLWP4A77TGZNhiGBkdCdmWasbRcmge";

/**
 * RPC endpoint. The public mainnet-beta endpoint works but is rate-limited
 * and can be flaky under load. For anything beyond a one-off transfer,
 * swap in a private RPC URL (Helius, QuickNode, Triton, etc.).
 */
export const RPC_ENDPOINT = "https://api.mainnet-beta.solana.com";
