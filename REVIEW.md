# Playa parts:
  * SceneGraph:
    - Node: A DAG node that can hold other Nodes, parent-child hierarchy
      - Layer (RefLayer): holds UUID of other Layer / Comp / Media file and a dict of it's attributes.
        - Media file: any file with UUID, one of possible "frame source". Others are "live", "procedural"
        - Text
        - Object (2d, 3d)
        - Audio

  * Project: Holds scene graph, including source media files
    - Timeline
    - Node View
    - Attribute editor
    - Viewport
    - Persistent Queue (Jobs, AI, Renders)

