0. Remove Comp::cache entirely
2. global cache with `comp_uuid+frame` keys.
3. different caching strategies: just last or all.
4. file comps are not "loading" frames anymore, they're caching them
5. single "dirty" attr on Attrs - when something was edited, .set() - Attrs become dirty -> recompose and recache.
6. Comp.get_frame():
  - calculates mega_uuid hash key for current comp uuid + frame
  - checks comp_attrs.dirty:
  - checks PlayaApp::cache[mega_uuid]
  - if dirty or no cache exists: compose() -> cache[mega_uuid]
  - else just get cache[mega_uuid] and return
7. All comps do that to other nested comps. File comps work the same - they're loading frame and caching it.
8. Upon getting to max_frames or max_mem - oldest frames are getting off. Maybe need to implement set/get/del/pop_lru methods for Cache structure as an interface

