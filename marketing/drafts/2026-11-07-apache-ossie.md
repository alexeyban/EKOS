EKOS now speaks Apache Ossie, the open semantic-model format formerly called OSI.

Your semantic layer, BI tool and AI agent each keep their own copy of what your tables mean. Each copy is retyped by hand, drifts from the schema, and disagrees with the others about what status 'C' means.

EKOS compiles the real schema, so it can write that model for you, and keep the ones you already have honest.

Export: ekos export ossie
• every table and view becomes a dataset with typed fields
• composite primary and unique keys, and foreign keys as relationships
• business meanings recovered from code (CHECK constraints, column comments, view filters), each one marked "unreviewed" until a person confirms it
• no invented metrics: a metric appears only if your own Ossie file defines it

Import, both ways:
• Ossie files in your repo are read as sources. Each dataset links to its table, and anything the file and the schema disagree on becomes a conflict with file and line: a field naming a dropped column, or a type the schema contradicts.
• A domain expert reviews meanings in the exported file. ekos import ossie turns the edits into human-only review decisions, all or nothing.

Agents get it through MCP (ekos_ossie_export), and search finds tables by the business synonyms in your model.

Checked against Ossie's own validator:
0. the validator rejects a deliberately broken model
1. export → validate
2. import Ossie's official TPC-DS model → export again → validate
3. review in the file → import → export → validate
4. planted schema drift → exactly those conflicts reported

All green, in CI against the pinned spec and Ossie's main every week. Run it yourself: scripts/ossie-validate.sh

On main now (spec 0.2.0.dev0, reads 0.1.1 too); coming in the next release.

Docs and use cases: https://ekos.dev/docs/integrations/apache-ossie.html
Open source (MIT): https://github.com/alexeyban/EKOS

#ApacheOssie #SemanticLayer #DataEngineering #dbt #Snowflake #MCP
