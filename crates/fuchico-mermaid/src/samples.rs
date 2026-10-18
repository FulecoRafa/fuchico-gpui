//! Sample diagrams: the one in the demo notes plus one per supported type.

pub const SAMPLES: &[(&str, &str)] = &[
    (
        "demo-notes: flowchart with markdown strings",
        "flowchart LR\n    markdown[\"`This **is** _Markdown_`\"]\n    newLines[\"`Line1\n    Line 2\n    Line 3`\"]\n    markdown --> newLines\n ",
    ),
    (
        "flowchart",
        "flowchart TD\n    A[Start] --> B{Is it working?}\n    B -->|Yes| C[Ship it]\n    B -->|No| D[Debug]\n    D --> B\n    subgraph Release\n      C --> E((Done))\n    end",
    ),
    (
        "sequence",
        "sequenceDiagram\n    participant U as User\n    participant A as App\n    participant S as Server\n    U->>A: Open note\n    A->>S: GET /note\n    S-->>A: 200 OK\n    A-->>U: Render\n    Note over U,A: live preview",
    ),
    (
        "class",
        "classDiagram\n    class Animal {\n      +String name\n      +eat()\n    }\n    class Dog {\n      +bark()\n    }\n    Animal <|-- Dog",
    ),
    (
        "state",
        "stateDiagram-v2\n    [*] --> Idle\n    Idle --> Running : start\n    Running --> Idle : stop\n    Running --> [*] : crash",
    ),
    (
        "er",
        "erDiagram\n    CUSTOMER ||--o{ ORDER : places\n    ORDER ||--|{ LINE_ITEM : contains\n    CUSTOMER {\n      string name\n      string email\n    }",
    ),
    (
        "gantt",
        "gantt\n    title Plan\n    dateFormat YYYY-MM-DD\n    section Build\n    Design :a1, 2026-01-01, 10d\n    Code   :after a1, 15d\n    section Ship\n    Release :2026-02-01, 5d",
    ),
    (
        "pie",
        "pie title Pets\n    \"Dogs\" : 386\n    \"Cats\" : 85\n    \"Rats\" : 15",
    ),
    (
        "mindmap",
        "mindmap\n  root((Fuchico))\n    Notes\n      Markdown\n      Wikilinks\n    Diagrams\n      Mermaid\n      Excalidraw",
    ),
    (
        "timeline",
        "timeline\n    title History\n    2024 : Idea\n    2025 : Tauri app\n    2026 : GPUI port",
    ),
    (
        "journey",
        "journey\n    title My day\n    section Work\n      Write code: 5: Me\n      Review: 3: Me",
    ),
    (
        "gitgraph",
        "gitGraph\n    commit\n    branch dev\n    checkout dev\n    commit\n    checkout main\n    merge dev",
    ),
];
