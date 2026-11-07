from typing import Tuple

from sparqlslurper import SlurpyGraph, TM_NS

graphdb_id = "<http://www.ontotext.com/owlim/entity#id>"


class GraphDBSlurpyGraph(SlurpyGraph):

    def gen_query(self,  pattern, gquery: str, gqueryend: str) -> str:
        """ Generate a query that includes the identifiers of any variables and adds identifier translation for
        any URI's
        """
        def gen_s_o(p: str, is_s: bool) -> Tuple[str, str]:
            tgt = '?s' if is_s else '?o'
            if p is None:
                return tgt, f' {tgt} {graphdb_id} {tgt}id . '
            elif str(p).startswith(str(TM_NS)):
                bnode_id = str(p)[len(str(TM_NS)):]
                return tgt, f' {tgt} {graphdb_id} {bnode_id} . '
            else:
                return self._repr_element(p), ''

        subj, s_add = gen_s_o(pattern[0], True)
        pred = self._repr_element(pattern[1]) if pattern[1] is not None else '?p'
        obj, o_add = gen_s_o(pattern[2], False)
        return f"SELECT ?s ?p ?o ?sid ?oid {{{gquery}{subj} {pred} {obj} . {s_add}{o_add} {gqueryend}}}"
