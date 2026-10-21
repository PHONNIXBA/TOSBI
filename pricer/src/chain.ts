import {
  Account,
  Address,
  Asset,
  BASE_FEE,
  Contract,
  Keypair,
  Operation,
  rpc,
  scValToNative,
  TransactionBuilder,
  xdr,
} from "@stellar/stellar-sdk";

export interface Net {
  rpcUrl: string;
  passphrase: string;
}

export const ERRORS = [
  "",
  "InvalidAmount",
  "InvalidQuote",
  "QuoteExpired",
  "NonceUsed",
  "Unauthorized",
  "AmountOutOfRange",
  "InsufficientCapital",
  "WalletCapExceeded",
  "NotFound",
  "AlreadyReleased",
  "WithdrawalPending",
  "NoWithdrawal",
  "WithdrawalLocked",
  "PaymentFailed",
];

export class Rejected extends Error {
  name: string;
  needs: string[];
  constructor(raw: string, needs: string[] = []) {
    super(raw);
    const code = Number(raw.match(/Error\(Contract, #(\d+)\)/)?.[1]);
    this.name = needs.length ? "AuthRequired" : (ERRORS[code] ?? "Failed");
    this.needs = needs;
  }
}

const READER = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF";
const server = (n: Net) => new rpc.Server(n.rpcUrl, { allowHttp: n.rpcUrl.startsWith("http://") });

function builder(n: Net, account: Account) {
  return new TransactionBuilder(account, { fee: BASE_FEE, networkPassphrase: n.passphrase }).setTimeout(120);
}

export async function read(n: Net, contract: string, method: string, args: xdr.ScVal[] = []): Promise<unknown> {
  const tx = builder(n, new Account(READER, "0")).addOperation(new Contract(contract).call(method, ...args)).build();
  const sim = await server(n).simulateTransaction(tx);
  if (rpc.Api.isSimulationError(sim)) throw new Rejected(sim.error);
  return sim.result ? scValToNative(sim.result.retval) : undefined;
}

function authAddress(entry: xdr.SorobanAuthorizationEntry, source: string): string {
  const c = entry.credentials;
  const creds =
    c.type === "sorobanCredentialsAddress" ? c.address
    : c.type === "sorobanCredentialsAddressV2" ? c.addressV2
    : c.type === "sorobanCredentialsAddressWithDelegates" ? c.addressWithDelegates.addressCredentials
    : null;
  return creds ? Address.fromScAddress(creds.address).toString() : source;
}

/** Simulates `method` as `source` and returns an assembled, unsigned transaction. */
export async function prepareInvoke(n: Net, source: string, contract: string, method: string, args: xdr.ScVal[]): Promise<string> {
  const s = server(n);
  const tx = builder(n, await s.getAccount(source)).addOperation(new Contract(contract).call(method, ...args)).build();
  const sim = await s.simulateTransaction(tx);
  if (rpc.Api.isSimulationError(sim)) throw new Rejected(sim.error);
  const needs = (sim.result?.auth ?? []).map((a) => authAddress(a, source)).filter((a) => a !== source);
  if (needs.length) throw new Rejected("requires another signer", needs);
  return rpc.assembleTransaction(tx, sim).build().toXDR();
}

export async function prepareTrust(n: Net, source: string, asset: Asset): Promise<string> {
  const account = await server(n).getAccount(source);
  return builder(n, account).addOperation(Operation.changeTrust({ asset })).build().toXDR();
}

export interface Receipt {
  hash: string;
  status: string;
  result?: unknown;
}

export async function submit(n: Net, signedXdr: string): Promise<Receipt> {
  const s = server(n);
  const sent = await s.sendTransaction(TransactionBuilder.fromXDR(signedXdr, n.passphrase));
  if (sent.status !== "PENDING" && sent.status !== "DUPLICATE") return { hash: sent.hash, status: sent.status };
  const got = await s.pollTransaction(sent.hash, { attempts: 20 });
  if (got.status !== rpc.Api.GetTransactionStatus.SUCCESS) return { hash: sent.hash, status: got.status };
  return { hash: sent.hash, status: got.status, result: got.returnValue ? scValToNative(got.returnValue) : undefined };
}

export async function pay(n: Net, secret: string, asset: Asset, to: string, amount: string): Promise<Receipt> {
  const kp = Keypair.fromSecret(secret);
  const tx = builder(n, await server(n).getAccount(kp.publicKey()))
    .addOperation(Operation.payment({ destination: to, asset, amount }))
    .build();
  tx.sign(kp);
  return submit(n, tx.toXDR());
}
