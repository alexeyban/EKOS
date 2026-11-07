from ._slurpygraph import QueryTriple, NodeType, RDFTriple, TM_NS, SlurpyGraph, QueryResultHook, QueryResultPrinter
from ._graphdb_slurpygraph import GraphDBSlurpyGraph, graphdb_id
from ._user_agent import SlurpyGraphWithAgent, SPARQLWrapperWithAgent

import rdflib_shim                      # rdflib 5 / 6 bridge
shimed = rdflib_shim.RDFLIB_SHIM        # Make sure the import doesn't get stripped