/** OpenCode resolves a provider factory before running its language hook. This local
 * factory cannot make a request if that hook is absent or an upstream API changes. */
export function createBrainProvider() {
  return { languageModel(): never { throw new Error("Brain's OpenCode model adapter is not installed"); } };
}
