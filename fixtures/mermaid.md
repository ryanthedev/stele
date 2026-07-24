# Mermaid fences

A flowchart with a decision node, rendered as a text block:

```mermaid
graph TD
  A[Start] --> B{Decision}
  B -->|Yes| C[Do thing]
  B -->|No| D[Skip]
```

An unsupported diagram type, which must fall back to a plain code block:

```mermaid
unknownDiagramType title Roadmap
```
