import type { EnvironmentControlRequest, EnvironmentRef, EnvironmentView } from "./generated/session.js";

export type { EnvironmentControlRequest, EnvironmentRef, EnvironmentView, EnvironmentGrant, EnvironmentLifecycle, EnvironmentMethod, EnvironmentObservation, EnvironmentOutput, EnvironmentTemplate } from "./generated/session.js";

/** Scoped to the extension operation that supplies the transport. */
export class EnvironmentServices {
  constructor(private readonly request: (request: EnvironmentControlRequest) => Promise<unknown>) {}

  list(): Promise<EnvironmentView[]> { return this.request({ operation: "list" }) as Promise<EnvironmentView[]>; }
  get(environment: EnvironmentRef): Promise<EnvironmentView> { return this.request({ operation: "get", environment }) as Promise<EnvironmentView>; }
  create(template: string, name: string, configuration: unknown): Promise<EnvironmentView> {
    return this.request({ operation: "create", template, name, configuration }) as Promise<EnvironmentView>;
  }
  setup(environment: EnvironmentRef): Promise<EnvironmentView> { return this.request({ operation: "setup", environment }) as Promise<EnvironmentView>; }
  update(environment: EnvironmentRef, configuration: unknown): Promise<EnvironmentView> {
    return this.request({ operation: "update", environment, configuration }) as Promise<EnvironmentView>;
  }
  delete(environment: EnvironmentRef): Promise<EnvironmentView> { return this.request({ operation: "delete", environment }) as Promise<EnvironmentView>; }
  call(environment: EnvironmentRef, method: string, input: unknown): Promise<unknown> {
    return this.request({ operation: "call", environment, method, input });
  }
}
