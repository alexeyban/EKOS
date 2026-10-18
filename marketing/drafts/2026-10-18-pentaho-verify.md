Still running Pentaho (Kettle) jobs? I need your help testing something.

EKOS Lakehouse now translates a whole Pentaho package — .ktr, .kjb, repository exports, shared.xml, JavaScript and Java steps — into dbt models on an Iceberg lakehouse: silver for the cleansing, gold for what each transformation writes.

It doesn't pretend. Every step gets a label:
• exact — same semantics in SQL
• rewritten — a deterministic rule you should read (Kettle treats '' as null, compares nulls as values, groups only consecutive rows…)
• llm-drafted — a model wrote it, and it was accepted only because it reads nothing but that step's own inputs and literals
• gap — not translated; the original code is kept and every model that depends on it is disabled

Trino plans every model before anything is proposed, and it lands on a git branch. Merging is the approval — EKOS never merges for you.

What I can't prove alone: that the dbt models return the same rows as your real transformations on your real data.

If you have a Pentaho estate and an hour:
1. ekos-lakehouse lakehouse pentaho translate <package>
2. run the original with pan.sh into a scratch schema
3. dbt build the Pentaho models
4. compare in Trino (two queries, in the guide)

Send counts, never data. A mismatch is the most useful result you can send me.

Guide: https://github.com/alexeyban/ekos-lakehouse/blob/main/docs/pentaho-verify.md
Open source (MIT): https://github.com/alexeyban/ekos-lakehouse

#Pentaho #Kettle #dbt #DataEngineering #ApacheIceberg #Trino
