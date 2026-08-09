# Architecture

## Components

- **Crate**: 39 — see below, `## Crate & Workspace Topology`
- **Document**: 13
- **File**: 767
- **Person**: 2
- **Pipeline**: 2 — see below, `## CI/CD Pipelines`
- **PythonModule**: 3 — see [API.md](API.md)
- **PythonSymbol**: 3 — see [API.md](API.md)
- **RustModule**: 446 — see [API.md](API.md)
- **RustSymbol**: 1326 — see [API.md](API.md)
- **Section**: 1577
- **Table**: 19
- **Technology**: 36 — see below, `## Technologies`
- **TransformNode**: 34

## Crate & Workspace Topology

```mermaid
graph TD
    n20c26e43ee965ee7a04625740fbbd56b["ekos-benchmark"]
    n8806bf5463645052b85acef4344e8f19["ekos-artifact"]
    n20c26e43ee965ee7a04625740fbbd56b -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    n2690bc0d0233516fb9699e87432f623d["ekos-compiler-core"]
    n20c26e43ee965ee7a04625740fbbd56b -->|DependsOn| n2690bc0d0233516fb9699e87432f623d
    n2c6b8d9a83ed510ea5d8a76f2e8685fe["ekos-identity"]
    n20c26e43ee965ee7a04625740fbbd56b -->|DependsOn| n2c6b8d9a83ed510ea5d8a76f2e8685fe
    n7e3bc0ded88855cdaa9af333f6e2cbb2["ekos-kir"]
    n20c26e43ee965ee7a04625740fbbd56b -->|DependsOn| n7e3bc0ded88855cdaa9af333f6e2cbb2
    n9c977335c421519ca889558f0487574e["ekos-ledger"]
    n20c26e43ee965ee7a04625740fbbd56b -->|DependsOn| n9c977335c421519ca889558f0487574e
    n9a955a3a55bc587a942dfb81d6260052["ekos-observation-sdk"]
    n20c26e43ee965ee7a04625740fbbd56b -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    ndf977fc8e004518eb267581520ccd448["ekos-plugin-git"]
    n20c26e43ee965ee7a04625740fbbd56b -->|DependsOn| ndf977fc8e004518eb267581520ccd448
    n28244ebb4e165e8da6375e750d01f2b8["ekos-recovery"]
    n20c26e43ee965ee7a04625740fbbd56b -->|DependsOn| n28244ebb4e165e8da6375e750d01f2b8
    nf4cd2d3bf0b05234ab2b39de3275d717["ekos-runtime"]
    n20c26e43ee965ee7a04625740fbbd56b -->|DependsOn| nf4cd2d3bf0b05234ab2b39de3275d717
    nf82d9ce0df2a5af89f9a2bd5f8484839["ekos-semantic"]
    n20c26e43ee965ee7a04625740fbbd56b -->|DependsOn| nf82d9ce0df2a5af89f9a2bd5f8484839
    n063808f95f195d62b3dd69eaa93d44cb["ekos-integration-tests"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0["ekos"]
    n063808f95f195d62b3dd69eaa93d44cb -->|DependsOn| nabd31cd9b31d54c587cd8a4a5b9a30a0
    n063808f95f195d62b3dd69eaa93d44cb -->|DependsOn| n2690bc0d0233516fb9699e87432f623d
    n063808f95f195d62b3dd69eaa93d44cb -->|DependsOn| n9c977335c421519ca889558f0487574e
    n063808f95f195d62b3dd69eaa93d44cb -->|DependsOn| nf4cd2d3bf0b05234ab2b39de3275d717
    n2c6b8d9a83ed510ea5d8a76f2e8685fe -->|DependsOn| n7e3bc0ded88855cdaa9af333f6e2cbb2
    nf82d9ce0df2a5af89f9a2bd5f8484839 -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    ndc169f0a98f15c7c8dd01dbc8504e9c9["ekos-common"]
    nf82d9ce0df2a5af89f9a2bd5f8484839 -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    nf82d9ce0df2a5af89f9a2bd5f8484839 -->|DependsOn| n2690bc0d0233516fb9699e87432f623d
    nf82d9ce0df2a5af89f9a2bd5f8484839 -->|DependsOn| n2c6b8d9a83ed510ea5d8a76f2e8685fe
    nf82d9ce0df2a5af89f9a2bd5f8484839 -->|DependsOn| n7e3bc0ded88855cdaa9af333f6e2cbb2
    nf4cd2d3bf0b05234ab2b39de3275d717 -->|DependsOn| n2690bc0d0233516fb9699e87432f623d
    nf4cd2d3bf0b05234ab2b39de3275d717 -->|DependsOn| n7e3bc0ded88855cdaa9af333f6e2cbb2
    nf4cd2d3bf0b05234ab2b39de3275d717 -->|DependsOn| n9c977335c421519ca889558f0487574e
    nf4cd2d3bf0b05234ab2b39de3275d717 -->|DependsOn| n28244ebb4e165e8da6375e750d01f2b8
    n2690bc0d0233516fb9699e87432f623d -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    n2690bc0d0233516fb9699e87432f623d -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    n2690bc0d0233516fb9699e87432f623d -->|DependsOn| n7e3bc0ded88855cdaa9af333f6e2cbb2
    n7e3bc0ded88855cdaa9af333f6e2cbb2 -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n2690bc0d0233516fb9699e87432f623d
    n9b66a043a00958d6b44620001b04c706["ekos-dbt-gen"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n9b66a043a00958d6b44620001b04c706
    nee66e2d3bd7f53c2a9f97dcb7cba59b3["ekos-docs-gen"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| nee66e2d3bd7f53c2a9f97dcb7cba59b3
    nd932eaf470695419a00cfa4b7b374c86["ekos-ekl"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| nd932eaf470695419a00cfa4b7b374c86
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n2c6b8d9a83ed510ea5d8a76f2e8685fe
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n7e3bc0ded88855cdaa9af333f6e2cbb2
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n9c977335c421519ca889558f0487574e
    n18dba45d95345035bd6fdf6b370079ac["ekos-marketing"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n18dba45d95345035bd6fdf6b370079ac
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    ne8d1a3c9e7b25084bdfc569e7b604054["ekos-plugin-confluence"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| ne8d1a3c9e7b25084bdfc569e7b604054
    n835d6e675bb053e79104338881612548["ekos-plugin-crypto"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n835d6e675bb053e79104338881612548
    n06b65958abb75fe1a6eed35946d39062["ekos-plugin-file"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n06b65958abb75fe1a6eed35946d39062
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| ndf977fc8e004518eb267581520ccd448
    naff4d491b33f56d7b0bae03e884983fd["ekos-plugin-github"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| naff4d491b33f56d7b0bae03e884983fd
    n0659fbf3d2f454ba835fa7f6f875a7d1["ekos-plugin-localdocs"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n0659fbf3d2f454ba835fa7f6f875a7d1
    ndac1d743a50e57a98acb56e29a47ef5e["ekos-plugin-pentaho"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| ndac1d743a50e57a98acb56e29a47ef5e
    n020c78caf337542fb3518b4201393bbb["ekos-plugin-python"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n020c78caf337542fb3518b4201393bbb
    n07179babd4865b148c686e743e45b3f6["ekos-plugin-rust"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n07179babd4865b148c686e743e45b3f6
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n28244ebb4e165e8da6375e750d01f2b8
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| nf4cd2d3bf0b05234ab2b39de3275d717
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| nf82d9ce0df2a5af89f9a2bd5f8484839
    n9a955a3a55bc587a942dfb81d6260052 -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    n9a955a3a55bc587a942dfb81d6260052 -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    n7bea4b92902b50728b4f740613d85745["ekos-compiler-sdk"]
    n7bea4b92902b50728b4f740613d85745 -->|DependsOn| n2690bc0d0233516fb9699e87432f623d
    n8806bf5463645052b85acef4344e8f19 -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    n8806bf5463645052b85acef4344e8f19 -->|DependsOn| n7e3bc0ded88855cdaa9af333f6e2cbb2
    n9b66a043a00958d6b44620001b04c706 -->|DependsOn| n7e3bc0ded88855cdaa9af333f6e2cbb2
    n18dba45d95345035bd6fdf6b370079ac -->|DependsOn| n2690bc0d0233516fb9699e87432f623d
    n18dba45d95345035bd6fdf6b370079ac -->|DependsOn| n28244ebb4e165e8da6375e750d01f2b8
    n2053b72d2c1851e486cdc9a252fd7f89["ekos-scheduler"]
    n2053b72d2c1851e486cdc9a252fd7f89 -->|DependsOn| n2690bc0d0233516fb9699e87432f623d
    nd932eaf470695419a00cfa4b7b374c86 -->|DependsOn| n7e3bc0ded88855cdaa9af333f6e2cbb2
    nd932eaf470695419a00cfa4b7b374c86 -->|DependsOn| nf4cd2d3bf0b05234ab2b39de3275d717
    nee66e2d3bd7f53c2a9f97dcb7cba59b3 -->|DependsOn| n7e3bc0ded88855cdaa9af333f6e2cbb2
    n28244ebb4e165e8da6375e750d01f2b8 -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    n28244ebb4e165e8da6375e750d01f2b8 -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    n28244ebb4e165e8da6375e750d01f2b8 -->|DependsOn| n2690bc0d0233516fb9699e87432f623d
    n28244ebb4e165e8da6375e750d01f2b8 -->|DependsOn| n7e3bc0ded88855cdaa9af333f6e2cbb2
    n920f420348d45079a5ee41b212c4858c["ekos-plugin-sql-dialect-databricks"]
    n28244ebb4e165e8da6375e750d01f2b8 -->|DependsOn| n920f420348d45079a5ee41b212c4858c
    n05ad9d89d39c5316b4132903b6b557db["ekos-plugin-sql-dialect-mssql"]
    n28244ebb4e165e8da6375e750d01f2b8 -->|DependsOn| n05ad9d89d39c5316b4132903b6b557db
    n001696e194795c36ae5308898760049d["ekos-plugin-sql-dialect-mysql"]
    n28244ebb4e165e8da6375e750d01f2b8 -->|DependsOn| n001696e194795c36ae5308898760049d
    nff9a3a7c06105442ac0b210e45700aad["ekos-plugin-sql-dialect-postgres"]
    n28244ebb4e165e8da6375e750d01f2b8 -->|DependsOn| nff9a3a7c06105442ac0b210e45700aad
    n15989d4959f8564ea77dc90d2d87c80b["ekos-plugin-sql-dialect-snowflake"]
    n28244ebb4e165e8da6375e750d01f2b8 -->|DependsOn| n15989d4959f8564ea77dc90d2d87c80b
    n28244ebb4e165e8da6375e750d01f2b8 -->|DependsOn| nf82d9ce0df2a5af89f9a2bd5f8484839
    nbf4371bd7cee54d1945706a1079a38cf["ekos-sql-dialect-sdk"]
    n28244ebb4e165e8da6375e750d01f2b8 -->|DependsOn| nbf4371bd7cee54d1945706a1079a38cf
    n9c977335c421519ca889558f0487574e -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    n9c977335c421519ca889558f0487574e -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    n9c977335c421519ca889558f0487574e -->|DependsOn| n7e3bc0ded88855cdaa9af333f6e2cbb2
    n05ad9d89d39c5316b4132903b6b557db -->|DependsOn| nbf4371bd7cee54d1945706a1079a38cf
    n66e4bdc107c65f6e9150d6db731cf29d["ekos-plugin-oracle"]
    n66e4bdc107c65f6e9150d6db731cf29d -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    n66e4bdc107c65f6e9150d6db731cf29d -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    n66e4bdc107c65f6e9150d6db731cf29d -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    ne8d1a3c9e7b25084bdfc569e7b604054 -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    ne8d1a3c9e7b25084bdfc569e7b604054 -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    ne8d1a3c9e7b25084bdfc569e7b604054 -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    n0659fbf3d2f454ba835fa7f6f875a7d1 -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    n0659fbf3d2f454ba835fa7f6f875a7d1 -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    n0659fbf3d2f454ba835fa7f6f875a7d1 -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    n870bf8c45212524ca4426fe561baf29d["ekos-plugin-sap"]
    n870bf8c45212524ca4426fe561baf29d -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    n870bf8c45212524ca4426fe561baf29d -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    n870bf8c45212524ca4426fe561baf29d -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    naff4d491b33f56d7b0bae03e884983fd -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    naff4d491b33f56d7b0bae03e884983fd -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    naff4d491b33f56d7b0bae03e884983fd -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    ndac1d743a50e57a98acb56e29a47ef5e -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    ndac1d743a50e57a98acb56e29a47ef5e -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    ndac1d743a50e57a98acb56e29a47ef5e -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    n06b65958abb75fe1a6eed35946d39062 -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    n06b65958abb75fe1a6eed35946d39062 -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    n06b65958abb75fe1a6eed35946d39062 -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    n920f420348d45079a5ee41b212c4858c -->|DependsOn| nbf4371bd7cee54d1945706a1079a38cf
    n020c78caf337542fb3518b4201393bbb -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    n020c78caf337542fb3518b4201393bbb -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    n020c78caf337542fb3518b4201393bbb -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    nff9a3a7c06105442ac0b210e45700aad -->|DependsOn| nbf4371bd7cee54d1945706a1079a38cf
    ndf977fc8e004518eb267581520ccd448 -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    ndf977fc8e004518eb267581520ccd448 -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    ndf977fc8e004518eb267581520ccd448 -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    naeb0688d1d0058a5b6d6245dfefa74cf["ekos-plugin-fabric"]
    naeb0688d1d0058a5b6d6245dfefa74cf -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    naeb0688d1d0058a5b6d6245dfefa74cf -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    naeb0688d1d0058a5b6d6245dfefa74cf -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    n0a005794329c5fc3a395a5c55cf9cfcb["ekos-plugin-snowflake"]
    n0a005794329c5fc3a395a5c55cf9cfcb -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    n0a005794329c5fc3a395a5c55cf9cfcb -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    n0a005794329c5fc3a395a5c55cf9cfcb -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    na9e38433d55055238c134f5c31f4e742["ekos-plugin-salesforce"]
    na9e38433d55055238c134f5c31f4e742 -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    na9e38433d55055238c134f5c31f4e742 -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    na9e38433d55055238c134f5c31f4e742 -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    n835d6e675bb053e79104338881612548 -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    n835d6e675bb053e79104338881612548 -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    n835d6e675bb053e79104338881612548 -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    n001696e194795c36ae5308898760049d -->|DependsOn| nbf4371bd7cee54d1945706a1079a38cf
    n15989d4959f8564ea77dc90d2d87c80b -->|DependsOn| nbf4371bd7cee54d1945706a1079a38cf
    n07179babd4865b148c686e743e45b3f6 -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    n07179babd4865b148c686e743e45b3f6 -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    n07179babd4865b148c686e743e45b3f6 -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
```

## Technologies

- **serde_json** — used by: ekos-benchmark, ekos-identity, ekos-semantic, ekos-runtime, ekos-compiler-core, ekos-kir, ekos, ekos-observation-sdk, ekos-artifact, ekos-dbt-gen, ekos-common, ekos-marketing, ekos-ekl, ekos-docs-gen, ekos-recovery, ekos-ledger, ekos-plugin-oracle, ekos-plugin-confluence, ekos-plugin-localdocs, ekos-plugin-sap, ekos-plugin-github, ekos-plugin-pentaho, ekos-plugin-file, ekos-plugin-python, ekos-plugin-git, ekos-plugin-fabric, ekos-plugin-snowflake, ekos-plugin-salesforce, ekos-plugin-crypto, ekos-plugin-rust
- **sqlparser** — used by: ekos-benchmark, ekos-sql-dialect-sdk, ekos-recovery, ekos-plugin-sql-dialect-mssql, ekos-plugin-sql-dialect-databricks, ekos-plugin-sql-dialect-postgres, ekos-plugin-sql-dialect-mysql, ekos-plugin-sql-dialect-snowflake
- **tempfile** — used by: ekos-benchmark, ekos-integration-tests, ekos-plugin-localdocs
- **tokio** — used by: ekos-benchmark, ekos-integration-tests, ekos-compiler-core, ekos, ekos-observation-sdk, ekos-marketing, ekos-recovery, ekos-plugin-oracle, ekos-plugin-confluence, ekos-plugin-localdocs, ekos-plugin-sap, ekos-plugin-github, ekos-plugin-pentaho, ekos-plugin-file, ekos-plugin-python, ekos-plugin-git, ekos-plugin-fabric, ekos-plugin-snowflake, ekos-plugin-salesforce, ekos-plugin-crypto, ekos-plugin-rust
- **uuid** — used by: ekos-benchmark, ekos-semantic, ekos-kir, ekos, ekos-artifact, ekos-common, ekos-marketing, ekos-recovery, ekos-ledger, ekos-plugin-crypto
- **anyhow** — used by: ekos-integration-tests, ekos-compiler-core, ekos, ekos-marketing, ekos-recovery
- **thiserror** — used by: ekos-identity, ekos-semantic, ekos-runtime, ekos-compiler-core, ekos-kir, ekos-observation-sdk, ekos-artifact, ekos-common, ekos-marketing, ekos-ekl, ekos-recovery, ekos-ledger, ekos-plugin-oracle, ekos-plugin-confluence, ekos-plugin-localdocs, ekos-plugin-sap, ekos-plugin-github, ekos-plugin-pentaho, ekos-plugin-file, ekos-plugin-python, ekos-plugin-git, ekos-plugin-fabric, ekos-plugin-snowflake, ekos-plugin-salesforce, ekos-plugin-crypto, ekos-plugin-rust
- **async-trait** — used by: ekos-semantic, ekos-runtime, ekos-compiler-core, ekos-observation-sdk, ekos-marketing, ekos-recovery, ekos-plugin-oracle, ekos-plugin-confluence, ekos-plugin-localdocs, ekos-plugin-sap, ekos-plugin-github, ekos-plugin-pentaho, ekos-plugin-file, ekos-plugin-python, ekos-plugin-git, ekos-plugin-fabric, ekos-plugin-snowflake, ekos-plugin-salesforce, ekos-plugin-crypto, ekos-plugin-rust
- **chrono** — used by: ekos-semantic, ekos-runtime, ekos-compiler-core, ekos-kir, ekos, ekos-observation-sdk, ekos-artifact, ekos-common, ekos-marketing, ekos-recovery, ekos-ledger, ekos-plugin-git, ekos-plugin-crypto
- **tracing** — used by: ekos-semantic, ekos-compiler-core, ekos, ekos-artifact, ekos-marketing, ekos-recovery, ekos-ledger, ekos-plugin-oracle, ekos-plugin-confluence, ekos-plugin-localdocs, ekos-plugin-sap, ekos-plugin-github, ekos-plugin-pentaho, ekos-plugin-file, ekos-plugin-python, ekos-plugin-git, ekos-plugin-fabric, ekos-plugin-snowflake, ekos-plugin-salesforce, ekos-plugin-crypto, ekos-plugin-rust
- **hex** — used by: ekos-compiler-core, ekos-observation-sdk, ekos-artifact, ekos-common, ekos-recovery, ekos-ledger, ekos-plugin-localdocs, ekos-plugin-pentaho, ekos-plugin-file, ekos-plugin-python, ekos-plugin-rust
- **sha2** — used by: ekos-compiler-core, ekos-observation-sdk, ekos-artifact, ekos-common, ekos-marketing, ekos-recovery, ekos-ledger, ekos-plugin-localdocs, ekos-plugin-pentaho, ekos-plugin-file, ekos-plugin-python, ekos-plugin-rust
- **toml** — used by: ekos-compiler-core, ekos, ekos-recovery
- **walkdir** — used by: ekos-compiler-core, ekos, ekos-observation-sdk, ekos-plugin-localdocs, ekos-plugin-pentaho, ekos-plugin-file, ekos-plugin-python, ekos-plugin-rust
- **clap** — used by: ekos
- **dotenvy** — used by: ekos
- **zstd** — used by: ekos-artifact, ekos-common, ekos-ledger
- **base64** — used by: ekos-marketing
- **hmac** — used by: ekos-marketing
- **percent-encoding** — used by: ekos-marketing
- **rand** — used by: ekos-marketing
- **reqwest** — used by: ekos-marketing, ekos-recovery, ekos-plugin-confluence, ekos-plugin-sap, ekos-plugin-github, ekos-plugin-fabric, ekos-plugin-snowflake, ekos-plugin-salesforce
- **glob** — used by: ekos-recovery
- **roxmltree** — used by: ekos-recovery
- **rustpython-ast** — used by: ekos-recovery
- **syn** — used by: ekos-recovery
- **memmap2** — used by: ekos-ledger
- **rusqlite** — used by: ekos-ledger
- **tantivy** — used by: ekos-ledger
- **docx-rs** — used by: ekos-plugin-localdocs
- **html2text** — used by: ekos-plugin-localdocs
- **lopdf** — used by: ekos-plugin-localdocs
- **mail-parser** — used by: ekos-plugin-localdocs
- **pdf-extract** — used by: ekos-plugin-localdocs
- **zip** — used by: ekos-plugin-localdocs
- **parquet** — used by: ekos-plugin-crypto

## CI/CD Pipelines

### Deploy Pages

Triggers: `push`, `workflow_dispatch`

- **deploy**
  - actions/checkout@v7
  - actions/upload-pages-artifact@v3
  - actions/deploy-pages@v4

### CI

Triggers: `push`, `pull_request`

- **build-and-test**
  - actions/checkout@v7
  - Install Rust stable
  - Cache cargo registry
  - Build (all crates)
  - Test (unit + integration)
  - Clippy (no warnings)
  - Format check
- **benchmark**
  - actions/checkout@v7
  - Install Rust stable
  - Cache cargo registry
  - Run benchmarks
  - Upload benchmark report

## Entity Relationships

```mermaid
erDiagram
    "categories" }o--|| "categories" : references
    "products" }o--|| "categories" : references
    "orders" }o--|| "customers" : references
    "order_items" }o--|| "orders" : references
    "order_items" }o--|| "products" : references
    "payments" }o--|| "orders" : references
    "Employees" }o--|| "Employees" : references
    "Orders" }o--|| "Customers" : references
    "Orders" }o--|| "Employees" : references
    "Orders" }o--|| "Shippers" : references
    "Products" }o--|| "Categories" : references
    "Products" }o--|| "Suppliers" : references
    "'Order Details'" }o--|| "Orders" : references
    "'Order Details'" }o--|| "Products" : references
    "Territories" }o--|| "Region" : references
    "EmployeeTerritories" }o--|| "Employees" : references
    "EmployeeTerritories" }o--|| "Territories" : references
    "CustomerCustomerDemo" }o--|| "Customers" : references
    "CustomerCustomerDemo" }o--|| "CustomerDemographics" : references
```

## Dependency Graph

### Calls

_750 `Calls` relationships compiled — diagram omitted, too large to render usefully. First 15 shown below; every object's own detail page (linked) lists its full relationship set._

- [GitHubApiClient::list_files](rustsymbol-githubapiclient-list-files.md) → [GitHubApiClient::request](rustsymbol-githubapiclient-request.md)
- [GitHubApiClient::list_items](rustsymbol-githubapiclient-list-items.md) → [GitHubApiClient::list_files](rustsymbol-githubapiclient-list-files.md)
- [GitHubApiClient::list_items](rustsymbol-githubapiclient-list-items.md) → [GitHubApiClient::request](rustsymbol-githubapiclient-request.md)
- [PythonAnalyzerPass::run](rustsymbol-pythonanalyzerpass-run.md) → [parse_python_file](rustsymbol-parse-python-file.md)
- [parse_python_file](rustsymbol-parse-python-file.md) → [walk_top_level_statement](rustsymbol-walk-top-level-statement.md)
- [add_import](rustsymbol-add-import-89c6ca8d.md) → [python_module_kir_id](rustsymbol-python-module-kir-id.md)
- [walk_top_level_statement](rustsymbol-walk-top-level-statement.md) → [try_recognize_chain_statement](rustsymbol-try-recognize-chain-statement.md)
- [walk_top_level_statement](rustsymbol-walk-top-level-statement.md) → [add_symbol](rustsymbol-add-symbol-458e9ef2.md)
- [walk_top_level_statement](rustsymbol-walk-top-level-statement.md) → [add_import](rustsymbol-add-import-89c6ca8d.md)
- [try_recognize_chain_statement](rustsymbol-try-recognize-chain-statement.md) → [calls_to_nodes](rustsymbol-calls-to-nodes.md)
- [try_recognize_chain_statement](rustsymbol-try-recognize-chain-statement.md) → [linearize_chain](rustsymbol-linearize-chain.md)
- [linearize_chain](rustsymbol-linearize-chain.md) → [linearize_chain](rustsymbol-linearize-chain.md)
- [join_keys_from_on](rustsymbol-join-keys-from-on.md) → [keyword_arg](rustsymbol-keyword-arg.md)
- [join_keys_from_on](rustsymbol-join-keys-from-on.md) → [string_constant](rustsymbol-string-constant.md)
- [join_kind_from_how](rustsymbol-join-kind-from-how.md) → [keyword_arg](rustsymbol-keyword-arg.md)

### Contains

_2906 `Contains` relationships compiled — diagram omitted, too large to render usefully. First 15 shown below; every object's own detail page (linked) lists its full relationship set._

- tests/fixtures/sample_project/src/main.rs → [main](rustsymbol-main.md)
- ekos/plugins/github/src/lib.rs → [GitHubItem](rustsymbol-githubitem.md)
- ekos/plugins/github/src/lib.rs → [GitHubClientError](rustsymbol-githubclienterror.md)
- ekos/plugins/github/src/lib.rs → [GitHubClient](rustsymbol-githubclient.md)
- ekos/plugins/github/src/lib.rs → [GitHubApiClient](rustsymbol-githubapiclient.md)
- ekos/plugins/github/src/lib.rs → [GitHubApiClient::new](rustsymbol-githubapiclient-new.md)
- ekos/plugins/github/src/lib.rs → [GitHubApiClient::request](rustsymbol-githubapiclient-request.md)
- ekos/plugins/github/src/lib.rs → [GitHubApiClient::list_files](rustsymbol-githubapiclient-list-files.md)
- ekos/plugins/github/src/lib.rs → [GitHubApiClient::list_items](rustsymbol-githubapiclient-list-items.md)
- ekos/plugins/github/src/lib.rs → [MockGitHubClient](rustsymbol-mockgithubclient.md)
- ekos/plugins/github/src/lib.rs → [MockGitHubClient::new](rustsymbol-mockgithubclient-new.md)
- ekos/plugins/github/src/lib.rs → [MockGitHubClient::list_items](rustsymbol-mockgithubclient-list-items.md)
- ekos/plugins/github/src/lib.rs → [GitHubObserver](rustsymbol-githubobserver.md)
- ekos/plugins/github/src/lib.rs → [GitHubObserver::new](rustsymbol-githubobserver-new.md)
- ekos/plugins/github/src/lib.rs → [GitHubObserver::name](rustsymbol-githubobserver-name.md)

### CoupledWith

_744 `CoupledWith` relationships compiled — diagram omitted, too large to render usefully. First 15 shown below; every object's own detail page (linked) lists its full relationship set._

- demo/transcripts/act-1.md → demo/transcripts/act-4.md
- ekos/crates/compiler-core/src/pass.rs → ekos/crates/semantic/src/lib.rs
- ekos/Cargo.lock → ekos/crates/identity/src/lib.rs
- demo/headless.sh → demo/transcripts/act-4.md
- README.md → ekos/plugins/localdocs/Cargo.toml
- TODO.md → ekos/crates/cli/src/commands/build.rs
- ekos/crates/identity/src/lib.rs → ekos/crates/recovery/src/git_analyzer.rs
- benchmark/benches/ledger_write.rs → benchmark/benches/observation_git.rs
- docs/index.html → docs/presentations.html
- ekos/crates/recovery/src/lib.rs → ekos/plugins/localdocs/src/lib.rs
- ekos/Cargo.lock → ekos/crates/cli/src/commands/mod.rs
- docs/index.html → ekos/crates/identity/src/cross_system.rs
- ekos/crates/recovery/src/local_docs_analyzer.rs → ekos/plugins/localdocs/Cargo.toml
- ekos/Cargo.lock → ekos/crates/recovery/src/sql_transform_analyzer.rs
- benchmark/Cargo.lock → ekos/crates/ledger/src/fact_ledger.rs

### DependsOn

_1666 `DependsOn` relationships compiled — diagram omitted, too large to render usefully. First 15 shown below; every object's own detail page (linked) lists its full relationship set._

- ekos/plugins/github/src/lib.rs → [async_trait::async_trait](rustmodule-async-trait-async-trait.md)
- ekos/plugins/github/src/lib.rs → [ekos_artifact::ObservationArtifact](rustmodule-ekos-artifact-observationartifact.md)
- ekos/plugins/github/src/lib.rs → [ekos_observation_sdk::ObservationPackage](rustmodule-ekos-observation-sdk-observationpackage.md)
- ekos/plugins/github/src/lib.rs → [ekos_observation_sdk::ObserveError](rustmodule-ekos-observation-sdk-observeerror.md)
- ekos/plugins/github/src/lib.rs → [ekos_observation_sdk::Observer](rustmodule-ekos-observation-sdk-observer.md)
- ekos/plugins/github/src/lib.rs → [ekos_observation_sdk::ScanContext](rustmodule-ekos-observation-sdk-scancontext.md)
- ekos/plugins/github/src/lib.rs → [serde::Deserialize](rustmodule-serde-deserialize.md)
- ekos/plugins/github/src/lib.rs → [serde::Serialize](rustmodule-serde-serialize.md)
- ekos/plugins/github/src/lib.rs → [std::sync::Arc](rustmodule-std-sync-arc.md)
- ekos/plugins/github/src/lib.rs → [thiserror::Error](rustmodule-thiserror-error.md)
- ekos/crates/recovery/src/python_analyzer.rs → [async_trait::async_trait](rustmodule-async-trait-async-trait.md)
- ekos/crates/recovery/src/python_analyzer.rs → [ekos_artifact::ArtifactId](rustmodule-ekos-artifact-artifactid.md)
- ekos/crates/recovery/src/python_analyzer.rs → [ekos_compiler_core::pass::CompilerPass](rustmodule-ekos-compiler-core-pass-compilerpass.md)
- ekos/crates/recovery/src/python_analyzer.rs → [ekos_compiler_core::pass::PassContext](rustmodule-ekos-compiler-core-pass-passcontext.md)
- ekos/crates/recovery/src/python_analyzer.rs → [ekos_compiler_core::pass::PassError](rustmodule-ekos-compiler-core-pass-passerror.md)

### ForeignKey

```mermaid
graph TD
    n6794f52865834e8d89842358f199ce12["categories"]
    n6794f52865834e8d89842358f199ce12 -->|ForeignKey| n6794f52865834e8d89842358f199ce12
    n7c2e13d4535e435d992e0db2d71e2c6d["products"]
    n7c2e13d4535e435d992e0db2d71e2c6d -->|ForeignKey| n6794f52865834e8d89842358f199ce12
    nad94b10656104be3905ecf114df00129["orders"]
    ndc844ed6954d4cca8ae89597873eb56e["customers"]
    nad94b10656104be3905ecf114df00129 -->|ForeignKey| ndc844ed6954d4cca8ae89597873eb56e
    n08a0316015ed4f15bbc297c816faf313["order_items"]
    n08a0316015ed4f15bbc297c816faf313 -->|ForeignKey| nad94b10656104be3905ecf114df00129
    n08a0316015ed4f15bbc297c816faf313 -->|ForeignKey| n7c2e13d4535e435d992e0db2d71e2c6d
    nc663212aa5f94534a3a5a47fa4f31a9b["payments"]
    nc663212aa5f94534a3a5a47fa4f31a9b -->|ForeignKey| nad94b10656104be3905ecf114df00129
    n18ddc966658845b18a845a6ff92a9460["Employees"]
    n18ddc966658845b18a845a6ff92a9460 -->|ForeignKey| n18ddc966658845b18a845a6ff92a9460
    ncd8c2d9d78e5430daf8816be08817b86["Orders"]
    ne0c56e7596774432b53e177fb7ecbad3["Customers"]
    ncd8c2d9d78e5430daf8816be08817b86 -->|ForeignKey| ne0c56e7596774432b53e177fb7ecbad3
    ncd8c2d9d78e5430daf8816be08817b86 -->|ForeignKey| n18ddc966658845b18a845a6ff92a9460
    nb40ad1cbc0184fbb8cdc0ec91049655f["Shippers"]
    ncd8c2d9d78e5430daf8816be08817b86 -->|ForeignKey| nb40ad1cbc0184fbb8cdc0ec91049655f
    n42110141a81c441a97c1094420890bed["Products"]
    ncb4c8bc084294ee9b5778dfe6a4bedb7["Categories"]
    n42110141a81c441a97c1094420890bed -->|ForeignKey| ncb4c8bc084294ee9b5778dfe6a4bedb7
    nd7162ea30ab240b6b4e3a4d788138303["Suppliers"]
    n42110141a81c441a97c1094420890bed -->|ForeignKey| nd7162ea30ab240b6b4e3a4d788138303
    naad094bf64d5428b99a64192cef78a08["'Order Details'"]
    naad094bf64d5428b99a64192cef78a08 -->|ForeignKey| ncd8c2d9d78e5430daf8816be08817b86
    naad094bf64d5428b99a64192cef78a08 -->|ForeignKey| n42110141a81c441a97c1094420890bed
    nf1d6c69d10954e1ba7dbca8a1fd33179["Territories"]
    na98521ec744f4127b06f59f94062bf33["Region"]
    nf1d6c69d10954e1ba7dbca8a1fd33179 -->|ForeignKey| na98521ec744f4127b06f59f94062bf33
    nad1c159d481547e6993863834ea6bd1a["EmployeeTerritories"]
    nad1c159d481547e6993863834ea6bd1a -->|ForeignKey| n18ddc966658845b18a845a6ff92a9460
    nad1c159d481547e6993863834ea6bd1a -->|ForeignKey| nf1d6c69d10954e1ba7dbca8a1fd33179
    n06b01f7296b14a439e148b0abb2b8d14["CustomerCustomerDemo"]
    n06b01f7296b14a439e148b0abb2b8d14 -->|ForeignKey| ne0c56e7596774432b53e177fb7ecbad3
    nd3d7c029bf964160bb9c732b5f01fe2d["CustomerDemographics"]
    n06b01f7296b14a439e148b0abb2b8d14 -->|ForeignKey| nd3d7c029bf964160bb9c732b5f01fe2d
```

### OwnedBy

_205 `OwnedBy` relationships compiled — diagram omitted, too large to render usefully. First 15 shown below; every object's own detail page (linked) lists its full relationship set._

- unknown → alexeyban
- unknown → alexeyban
- unknown → alexeyban
- unknown → alexeyban
- unknown → alexeyban
- unknown → alexeyban
- unknown → alexeyban
- unknown → alexeyban
- unknown → alexeyban
- unknown → alexeyban
- unknown → alexeyban
- unknown → alexeyban
- unknown → alexeyban
- unknown → alexeyban
- unknown → alexeyban

