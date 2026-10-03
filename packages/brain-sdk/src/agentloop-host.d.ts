declare module "brain:agentloop/host@0.2.0" {
  export function model(requestJson: string): string;
  export function dispatch(callsJson: string): string;
  export function events(after: bigint): string;
  export function setTranscript(messagesJson: string): bigint;
  export function kvRead(key: string): string | undefined;
  export function kvPut(key: string, valueJson: string): bigint;
  export function kvDelete(key: string): bigint;
  export function emit(kind: string, payloadJson: string): bigint;
  export function environments(requestJson: string): string;
  export function telemetry(recordJson: string): void;
}
