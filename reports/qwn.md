# PLAYA Architecture Analysis

## Overview
This document analyzes the current PLAYA application architecture and compares it with the proposed architecture in arch.md to identify differences, improvements, and areas for refactoring.

## Current Architecture (Actual Codebase)

### Core Components

#### 1. **PlayaApp (main.rs)**
- Main application state with direct dependency injection
- Handles UI rendering, event processing, and application lifecycle
- Contains: Player, CacheManager, Workers, Project, EventBus, UI state
- Direct references to global components (cache, workers, etc.)
- Manual dependency injection rather than centralized service system

#### 2. **Project (entities/project.rs)**
- Top-level container with unified media pool (both File and Layer mode comps)
- Uses `Arc<RwLock<HashMap<Uuid, Comp>>>` for thread-safe media access
- Global frame cache shared across all comps
- Cascade invalidation for parent-child dependency updates

#### 3. **Comp (entities/comp.rs)**
- Dual-mode entity: File mode (loads sequences) or Layer mode (composes children)
- Uses Attrs for all persistent properties
- Global frame cache integration
- Recursive composition with CPU/GPU fallback
- Child management with timeline positioning

#### 4. **Attrs (entities/attrs.rs)**
- Generic key-value storage for entity metadata
- Dirty tracking with AtomicBool
- Hash-based caching invalidation
- All property-based attributes stored here

#### 5. **Event System (events.rs)**
- EventBus using crossbeam channels
- AppEvent for UI-driven events
- CompEvent for composition-level notifications
- Centralized event flow

#### 6. **Workers (workers.rs)**
- Work-stealing thread pool
- Epoch-based cancellation for stale requests
- Priority-based task execution

## Proposed Architecture (arch.md)

### Core Components

#### 1. **PlayaApp (main.rs)**
- Cleaner separation with centralized service container
- Explicit EventBus for communication
- Attrs-based global state management

#### 2. **Node (entities/node.rs)**
- New base entity pattern with Attrs and transient data
- Standardized schema initialization
- Centralized compute method

#### 3. **Comp (entities/comp.rs)**
- Specialized Node with nested container schema
- Improved time mapping between parent/child comps
- Better DAG relationship management

#### 4. **Project (entities/project.rs)**
- Simplified container with global cache management
- Clearer separation of concerns
- Better media management

## Key Differences and Improvements

### 1. **Centralization vs Decentralization**

**Current**: Components directly reference each other (Project has cache manager, Comps reference global cache)
**Proposed**: Centralized service architecture where components communicate via EventBus and central Attrs

### 2. **Node Abstraction**

**Current**: Comp is the base entity with dual modes
**Proposed**: Node base trait with specialized implementations (Comp, Frame, etc.)

### 3. **State Management**

**Current**: Mixed approach with direct field access and Attrs
**Proposed**: Attrs as the sole source of truth for all persistent state

### 4. **Event Communication**

**Current**: Direct method calls and event bus
**Proposed**: Strict EventBus-only communication for all interactions

### 5. **Caching Strategy**

**Current**: Global frame cache with epoch-based invalidation
**Proposed**: Enhanced caching with more sophisticated strategies and tracking

## Issues in Current Architecture

1. **Tight Coupling**: Components have direct dependencies on each other
2. **Inconsistent State Management**: Mix of direct fields and Attrs
3. **Complex Initialization**: Dependencies scattered across constructors
4. **Event Handling**: Mixed direct calls and event-based communication

## Benefits of Proposed Architecture

1. **Loose Coupling**: Components communicate only through EventBus
2. **Consistent State**: All state in Attrs with dirty tracking
3. **Testability**: Easier to mock services and test individual components
4. **Maintainability**: Clearer separation of concerns
5. **Scalability**: Better architecture for adding features

## Recommendations for Migration

### Phase 1: Node Abstraction
- Implement Node trait as base for all entities
- Gradually migrate Comp to use Node pattern
- Maintain backward compatibility during transition

### Phase 2: Event-Only Communication
- Replace direct method calls with event-based communication
- Enhance EventBus with better error handling and debugging
- Ensure all state changes go through events

### Phase 3: Centralized Services
- Create central service container/registry
- Implement dependency injection pattern
- Move to Attrs-only state management

### Phase 4: Refactoring
- Remove direct dependencies between components
- Implement proper service lifecycles
- Add comprehensive testing for new architecture

## MCP Implementation Notes

The proposed architecture aligns with modern Rust application patterns:
- **Event-driven architecture** reduces coupling
- **Service container pattern** improves testability
- **Attrs-based state management** provides consistent serialization
- **Work-stealing thread pool** maintains performance characteristics

The main changes needed are:
1. Implement the Node trait system
2. Refine the EventBus to handle all communication
3. Standardize on Attrs for all persistent state
4. Implement proper service dependency injection

This architecture would make the codebase more maintainable, testable, and scalable while preserving the performance characteristics of the current implementation.