from typing import Optional, List, Dict

from ShExJSG import ShExJ


class Schema(ShExJ.Schema):
    """ Wrapper for ShExJ Schema with the JSON-LD context element """
    def __init__(self,
                 imports: Optional[List[ShExJ.IRIREF]] = None,
                 startActs: Optional[List[ShExJ.SemAct]] = None,
                 start: Optional[ShExJ.shapeExpr] = None,
                 shapes: Optional[List[ShExJ.shapeExpr]] = None,
                 **_kwargs: Dict[str, object]):
        super().__init__(imports, startActs, start, shapes, **_kwargs)
        self['@context'] = "http://www.w3.org/ns/shex.jsonld"
