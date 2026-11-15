import { ManagedTool, type ManagedSelection } from './authoring.js';

export interface AwsS3Options {
  bucket?: string;
  region?: string;
  endpoint?: string;
  prefix?: string;
  operations?: ('list' | 'read' | 'write' | 'delete')[];
  accessKeyId?: string;
  secretAccessKey?: string;
  sessionToken?: string;
}

export interface ModalOptions {
  tokenId?: string;
  tokenSecret?: string;
  app?: string;
  image?: string;
  operations?: ('create' | 'exec' | 'inspect' | 'terminate')[];
}

export interface E2BOptions {
  apiKey?: string;
  template?: string;
  operations?: ModalOptions['operations'];
}

export interface DaytonaOptions {
  apiKey?: string;
  apiUrl?: string;
  target?: string;
  operations?: ModalOptions['operations'];
}

function integration(name: string, options: object, parameters: string[], secrets: string[], operations: string[]): ManagedSelection {
  const input = options as Record<string, unknown>;
  for (const key of Object.keys(input)) {
    if (![...parameters, ...secrets, 'operations'].includes(key)) throw new Error(`Unknown ${name} option: ${key}`);
  }
  const pick = (keys: string[]) => Object.fromEntries(keys.filter(key => input[key] !== undefined).map(key => [key, input[key]]));
  const credential = pick(secrets);
  return ManagedTool(name, {
    operations: (input.operations as string[] | undefined) ?? operations,
    parameters: pick(parameters),
    ...(Object.keys(credential).length ? { credential } : {}),
  });
}

/** Install S3 on a server, or select S3 with optional private session credentials. */
export const AwsS3 = (options: AwsS3Options = {}): ManagedSelection => integration('aws_s3', options,
  ['bucket', 'region', 'endpoint', 'prefix'], ['accessKeyId', 'secretAccessKey', 'sessionToken'], ['list', 'read']);

export const Modal = (options: ModalOptions = {}): ManagedSelection => integration('modal', options,
  ['app', 'image'], ['tokenId', 'tokenSecret'], ['create', 'exec', 'inspect', 'terminate']);

export const E2B = (options: E2BOptions = {}): ManagedSelection => integration('e2b', options,
  ['template'], ['apiKey'], ['create', 'exec', 'inspect', 'terminate']);

export const Daytona = (options: DaytonaOptions = {}): ManagedSelection => integration('daytona', options,
  ['apiUrl', 'target'], ['apiKey'], ['create', 'exec', 'inspect', 'terminate']);
