# Code design and patterns

1. **AI Coding Agent driven**
   1. Consistent design patterns, naming, folder structure, and abstractions for exposing complete relevant information when grep/ls etc.
   2. Explicit is better than implicit, do not let AI miss out context
   3. Prefer single source of truth (SSOT) to surface dependencies.
   4. Documentations and comments should be minimal, avoid knowledge easily inferred from codebase, they are easily stale and mislead AI
2. **Low coupling, high cohesion**
   1. Minimize dependencies between modules and classes
   2. Keep services independently changeable and deployable
   3. Keep responsibilities strongly related and focused
   4. Composability - build small and reusable parts
   5. Separation of Concerns
3. **DRY + AHA**
   1. Don't repeat yourself but avoid hasty abstraction
4. **Well Architected**
   1. Minimize room for errors by design
   2. Make illegal states unrepresentable
   3. Prefer modern, actively maintained libraries
   4. Minimize entities/terms with Occam's razor
   5. Minimize code change with https://github.com/DietrichGebert/ponytail/blob/main/skills/ponytail/SKILL.md
5. **APIs and interfaces**
   1. Principle of Least Astonishment - behave in ways that users and programmers naturally expect
   2. Fail fast - detect invalid assumptions immediately and visibly
   3. Open/Closed - open for extension, closed for modification
   4. Dependency inversion principle - high-level policy should depend on abstractions, not low-level details.
   5. Prefer small, pure, deterministic functions do one thing well, minimize error surfaces
   6. Avoid captive and rigid interfaces
   7. Encapsulation and abstraction - hide internal state and implementation details behind stable operations.
   8. Make public contracts stable while allowing internals to evolve.