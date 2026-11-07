import typing
import pyjsg.jsglib as jsg

# .TYPE and .IGNORE settings
_CONTEXT = jsg.JSGContext()
_CONTEXT.TYPE = "type"
_CONTEXT.TYPE_EXCEPTIONS.append("ObjectLiteral")
_CONTEXT.TYPE_EXCEPTIONS.append("InnerContext")


class _Anon1(jsg.JSGString):
    pattern = jsg.JSGPattern(r'http://www\.w3\.org/ns/shex\.jsonld')


class _Anon2(jsg.JSGString):
    pattern = jsg.JSGPattern(r'http://www\.w3\.org/ns/shex\.jsonld')


class _Anon3(jsg.JSGString):
    pattern = jsg.JSGPattern(r'(iri)|(bnode)|(nonliteral)|(literal)')


class LANGTAG(jsg.JSGString):
    pattern = jsg.JSGPattern(r'[a-zA-Z]+(\-([a-zA-Z0-9])+)*')


class PN_CHARS_BASE(jsg.JSGString):
    pattern = jsg.JSGPattern(r'[A-Z]|[a-z]|[\u00C0-\u00D6]|[\u00D8-\u00F6]|[\u00F8-\u02FF]|[\u0370-\u037D]|[\u037F-\u1FFF]|[\u200C-\u200D]|[\u2070-\u218F]|[\u2C00-\u2FEF]|[\u3001-\uD7FF]|[\uF900-\uFDCF]|[\uFDF0-\uFFFD]|[\u10000-\uEFFFF]')


class HEX(jsg.JSGString):
    pattern = jsg.JSGPattern(r'[0-9]|[A-F]|[a-f]')


class PN_CHARS_U(jsg.JSGString):
    pattern = jsg.JSGPattern(r'({PN_CHARS_BASE})|_'.format(PN_CHARS_BASE=PN_CHARS_BASE.pattern))


class UCHAR(jsg.JSGString):
    pattern = jsg.JSGPattern(r'\\\\u({HEX})({HEX})({HEX})({HEX})|\\\\U({HEX})({HEX})({HEX})({HEX})({HEX})({HEX})({HEX})({HEX})'.format(HEX=HEX.pattern))


class IRIREF(jsg.JSGString):
    pattern = jsg.JSGPattern(r'([^\u0000-\u0020\u005C\u007B\u007D<>"|^`]|({UCHAR}))*'.format(UCHAR=UCHAR.pattern))


class PN_CHARS(jsg.JSGString):
    pattern = jsg.JSGPattern(r'({PN_CHARS_U})|\-|[0-9]|\\u00B7|[\u0300-\u036F]|[\u203F-\u2040]'.format(PN_CHARS_U=PN_CHARS_U.pattern))


class BNODE(jsg.JSGString):
    pattern = jsg.JSGPattern(r'_:(({PN_CHARS_U})|[0-9])((({PN_CHARS})|\.)*({PN_CHARS}))?'.format(PN_CHARS=PN_CHARS.pattern, PN_CHARS_U=PN_CHARS_U.pattern))


class PN_PREFIX(jsg.JSGString):
    pattern = jsg.JSGPattern(r'({PN_CHARS_BASE})((({PN_CHARS})|\.)*({PN_CHARS}))?'.format(PN_CHARS=PN_CHARS.pattern, PN_CHARS_BASE=PN_CHARS_BASE.pattern))


class InnerContext(jsg.JSGObject):
    _reference_types = []
    _members = {}
    _strict = False

    def __init__(self,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)



class stringFacet_1_(jsg.JSGObject):
    _reference_types = []
    _members = {'length': jsg.Integer,
                'minlength': jsg.Integer,
                'maxlength': jsg.Integer}
    _strict = True

    def __init__(self,
                 length: int = None,
                 minlength: int = None,
                 maxlength: int = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.length = length
        self.minlength = minlength
        self.maxlength = maxlength



class stringFacet_2_(jsg.JSGObject):
    _reference_types = []
    _members = {'pattern': jsg.String,
                'flags': typing.Optional[jsg.String]}
    _strict = True

    def __init__(self,
                 pattern: str = None,
                 flags: typing.Optional[str] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.pattern = pattern
        self.flags = flags



class numericFacet(jsg.JSGObject):
    _reference_types = []
    _members = {'mininclusive': jsg.Number,
                'minexclusive': jsg.Number,
                'maxinclusive': jsg.Number,
                'maxexclusive': jsg.Number,
                'totaldigits': jsg.Integer,
                'fractiondigits': jsg.Integer}
    _strict = True

    def __init__(self,
                 mininclusive: float = None,
                 minexclusive: float = None,
                 maxinclusive: float = None,
                 maxexclusive: float = None,
                 totaldigits: int = None,
                 fractiondigits: int = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.mininclusive = mininclusive
        self.minexclusive = minexclusive
        self.maxinclusive = maxinclusive
        self.maxexclusive = maxexclusive
        self.totaldigits = totaldigits
        self.fractiondigits = fractiondigits



class LiteralStem(jsg.JSGObject):
    _reference_types = []
    _members = {'stem': jsg.String}
    _strict = True

    def __init__(self,
                 stem: str = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.stem = stem



class LanguageStem(jsg.JSGObject):
    _reference_types = []
    _members = {'stem': jsg.String}
    _strict = True

    def __init__(self,
                 stem: str = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.stem = stem



class Wildcard(jsg.JSGObject):
    _reference_types = []
    _members = {}
    _strict = True

    def __init__(self,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)



class xsFacet_2_(jsg.JSGObject):
    _reference_types = [numericFacet]
    _members = {'mininclusive': jsg.Number,
                'minexclusive': jsg.Number,
                'maxinclusive': jsg.Number,
                'maxexclusive': jsg.Number,
                'totaldigits': jsg.Integer,
                'fractiondigits': jsg.Integer}
    _strict = True

    def __init__(self,
                 mininclusive: float = None,
                 minexclusive: float = None,
                 maxinclusive: float = None,
                 maxexclusive: float = None,
                 totaldigits: int = None,
                 fractiondigits: int = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.mininclusive = mininclusive
        self.minexclusive = minexclusive
        self.maxinclusive = maxinclusive
        self.maxexclusive = maxexclusive
        self.totaldigits = totaldigits
        self.fractiondigits = fractiondigits



class stringFacet(jsg.JSGObject):
    _reference_types = [stringFacet_1_, stringFacet_2_]
    _members = {'length': typing.Optional[jsg.Integer],
                'minlength': typing.Optional[jsg.Integer],
                'maxlength': typing.Optional[jsg.Integer],
                'pattern': typing.Optional[jsg.String],
                'flags': typing.Optional[jsg.String]}
    _strict = True

    def __init__(self,
                 opts_: typing.Union[stringFacet_1_, stringFacet_2_] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        if opts_ is not None:
            if isinstance(opts_, stringFacet_1_):
                self.length = opts_.length
                self.minlength = opts_.minlength
                self.maxlength = opts_.maxlength
            elif isinstance(opts_, stringFacet_2_):
                self.pattern = opts_.pattern
                self.flags = opts_.flags
            else:
                raise ValueError(f"Unrecognized value type: {opts_}")



class LiteralStemRange(jsg.JSGObject):
    _reference_types = []
    _members = {'stem': typing.Union[jsg.String, Wildcard],
                'exclusions': jsg.ArrayFactory('exclusions', _CONTEXT, typing.Union[jsg.String, LiteralStem], 1, None)}
    _strict = True

    def __init__(self,
                 stem: typing.Union[str, Wildcard] = None,
                 exclusions: typing.List[typing.Union[str, LiteralStem]] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.stem = stem
        self.exclusions = exclusions



class Language(jsg.JSGObject):
    _reference_types = []
    _members = {'languageTag': LANGTAG}
    _strict = True

    def __init__(self,
                 languageTag: str = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.languageTag = languageTag



class LanguageStemRange(jsg.JSGObject):
    _reference_types = []
    _members = {'stem': typing.Union[jsg.String, Wildcard],
                'exclusions': jsg.ArrayFactory('exclusions', _CONTEXT, typing.Union[LANGTAG, LanguageStem], 1, None)}
    _strict = True

    def __init__(self,
                 stem: typing.Union[str, Wildcard] = None,
                 exclusions: typing.List[typing.Union[str, LanguageStem]] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.stem = stem
        self.exclusions = exclusions



class xsFacet_1_(jsg.JSGObject):
    _reference_types = [stringFacet]
    _members = {'length': typing.Optional[jsg.Integer],
                'minlength': typing.Optional[jsg.Integer],
                'maxlength': typing.Optional[jsg.Integer],
                'pattern': typing.Optional[jsg.String],
                'flags': typing.Optional[jsg.String]}
    _strict = True

    def __init__(self,
                 opts_: typing.Union[stringFacet_1_, stringFacet_2_] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        if opts_ is not None:
            if isinstance(opts_, stringFacet_1_):
                self.length = opts_.length
                self.minlength = opts_.minlength
                self.maxlength = opts_.maxlength
            elif isinstance(opts_, stringFacet_2_):
                self.pattern = opts_.pattern
                self.flags = opts_.flags
            else:
                raise ValueError(f"Unrecognized value type: {opts_}")



class xsFacet(jsg.JSGObject):
    _reference_types = [xsFacet_1_, xsFacet_2_]
    _members = {'length': typing.Optional[typing.Optional[jsg.Integer]],
                'minlength': typing.Optional[typing.Optional[jsg.Integer]],
                'maxlength': typing.Optional[typing.Optional[jsg.Integer]],
                'pattern': typing.Optional[typing.Optional[jsg.String]],
                'flags': typing.Optional[typing.Optional[jsg.String]],
                'mininclusive': typing.Optional[typing.Optional[jsg.Number]],
                'minexclusive': typing.Optional[typing.Optional[jsg.Number]],
                'maxinclusive': typing.Optional[typing.Optional[jsg.Number]],
                'maxexclusive': typing.Optional[typing.Optional[jsg.Number]],
                'totaldigits': typing.Optional[typing.Optional[jsg.Integer]],
                'fractiondigits': typing.Optional[typing.Optional[jsg.Integer]]}
    _strict = True

    def __init__(self,
                 opts_: typing.Union[xsFacet_1_, xsFacet_2_] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        if opts_ is not None:
            if isinstance(opts_, xsFacet_1_):
                if opts_ is not None:
                    if isinstance(opts_, stringFacet_1_):
                        self.length = opts_.length
                        self.minlength = opts_.minlength
                        self.maxlength = opts_.maxlength
                    elif isinstance(opts_, stringFacet_2_):
                        self.pattern = opts_.pattern
                        self.flags = opts_.flags
                    else:
                        raise ValueError(f"Unrecognized value type: {opts_}")
            elif isinstance(opts_, xsFacet_2_):
                self.mininclusive = opts_.mininclusive
                self.minexclusive = opts_.minexclusive
                self.maxinclusive = opts_.maxinclusive
                self.maxexclusive = opts_.maxexclusive
                self.totaldigits = opts_.totaldigits
                self.fractiondigits = opts_.fractiondigits
            else:
                raise ValueError(f"Unrecognized value type: {opts_}")



class ObjectLiteral(jsg.JSGObject):
    _reference_types = []
    _members = {'value': jsg.String,
                'language': typing.Optional[LANGTAG],
                'type': typing.Optional[IRIREF]}
    _strict = True

    def __init__(self,
                 value: str = None,
                 language: typing.Optional[str] = None,
                 type: typing.Optional[str] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.value = value
        self.language = language
        self.type = type



class IriStem(jsg.JSGObject):
    _reference_types = []
    _members = {'stem': IRIREF}
    _strict = True

    def __init__(self,
                 stem: str = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.stem = stem



class SemAct(jsg.JSGObject):
    _reference_types = []
    _members = {'name': IRIREF,
                'code': typing.Optional[jsg.String]}
    _strict = True

    def __init__(self,
                 name: str = None,
                 code: typing.Optional[str] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.name = name
        self.code = code



shapeExprLabel = typing.Union[IRIREF, BNODE]


objectValue = typing.Union[IRIREF, ObjectLiteral]


class IriStemRange(jsg.JSGObject):
    _reference_types = []
    _members = {'stem': typing.Union[IRIREF, Wildcard],
                'exclusions': jsg.ArrayFactory('exclusions', _CONTEXT, typing.Union[IRIREF, IriStem], 1, None)}
    _strict = True

    def __init__(self,
                 stem: typing.Union[str, Wildcard] = None,
                 exclusions: typing.List[typing.Union[str, IriStem]] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.stem = stem
        self.exclusions = exclusions



tripleExprLabel = typing.Union[IRIREF, BNODE]


class ShapeExternal(jsg.JSGObject):
    _reference_types = []
    _members = {'id': typing.Optional[shapeExprLabel]}
    _strict = True

    def __init__(self,
                 id: typing.Optional[typing.Union[str, str]] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.id = id



valueSetValue = typing.Union[typing.Union[IRIREF, ObjectLiteral], IriStem, IriStemRange, LiteralStem, LiteralStemRange, Language, LanguageStem, LanguageStemRange]


class Annotation(jsg.JSGObject):
    _reference_types = []
    _members = {'predicate': IRIREF,
                'object': objectValue}
    _strict = True

    def __init__(self,
                 predicate: str = None,
                 object: typing.Union[str, ObjectLiteral] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.predicate = predicate
        self.object = object



class NodeConstraint(jsg.JSGObject):
    _reference_types = [xsFacet]
    _members = {'id': typing.Optional[shapeExprLabel],
                'nodeKind': typing.Optional[_Anon3],
                'datatype': typing.Optional[IRIREF],
                'length': typing.Optional[typing.Optional[jsg.Integer]],
                'minlength': typing.Optional[typing.Optional[jsg.Integer]],
                'maxlength': typing.Optional[typing.Optional[jsg.Integer]],
                'pattern': typing.Optional[typing.Optional[jsg.String]],
                'flags': typing.Optional[typing.Optional[jsg.String]],
                'mininclusive': typing.Optional[typing.Optional[jsg.Number]],
                'minexclusive': typing.Optional[typing.Optional[jsg.Number]],
                'maxinclusive': typing.Optional[typing.Optional[jsg.Number]],
                'maxexclusive': typing.Optional[typing.Optional[jsg.Number]],
                'totaldigits': typing.Optional[typing.Optional[jsg.Integer]],
                'fractiondigits': typing.Optional[typing.Optional[jsg.Integer]],
                'values': typing.Optional[jsg.ArrayFactory('values', _CONTEXT, typing.Union[typing.Union[IRIREF, ObjectLiteral], IriStem, IriStemRange, LiteralStem, LiteralStemRange, Language, LanguageStem, LanguageStemRange], 1, None)]}
    _strict = True

    def __init__(self,
                 id: typing.Optional[typing.Union[str, str]] = None,
                 nodeKind: typing.Optional[str] = None,
                 datatype: typing.Optional[str] = None,
                 opts_: typing.Union[xsFacet_1_, xsFacet_2_] = None,
                 values: typing.Optional[typing.List[typing.Union[typing.Union[str, ObjectLiteral], IriStem, IriStemRange, LiteralStem, LiteralStemRange, Language, LanguageStem, LanguageStemRange]]] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.id = id
        self.nodeKind = nodeKind
        self.datatype = datatype
        if opts_ is not None:
            if isinstance(opts_, xsFacet_1_):
                if opts_ is not None:
                    if isinstance(opts_, stringFacet_1_):
                        self.length = opts_.length
                        self.minlength = opts_.minlength
                        self.maxlength = opts_.maxlength
                    elif isinstance(opts_, stringFacet_2_):
                        self.pattern = opts_.pattern
                        self.flags = opts_.flags
                    else:
                        raise ValueError(f"Unrecognized value type: {opts_}")
            elif isinstance(opts_, xsFacet_2_):
                self.mininclusive = opts_.mininclusive
                self.minexclusive = opts_.minexclusive
                self.maxinclusive = opts_.maxinclusive
                self.maxexclusive = opts_.maxexclusive
                self.totaldigits = opts_.totaldigits
                self.fractiondigits = opts_.fractiondigits
            else:
                raise ValueError(f"Unrecognized value type: {opts_}")
        self.values = values



class Schema(jsg.JSGObject):
    _reference_types = []
    _members = {'@context': typing.Union[_Anon2, jsg.ArrayFactory('@context', _CONTEXT, typing.Union[_Anon1, InnerContext], 0, None)],
                'imports': typing.Optional[jsg.ArrayFactory('imports', _CONTEXT, IRIREF, 1, None)],
                'startActs': typing.Optional[jsg.ArrayFactory('startActs', _CONTEXT, SemAct, 1, None)],
                'start': typing.Optional["shapeExpr"],
                'shapes': typing.Optional[jsg.ArrayFactory('shapes', _CONTEXT, typing.Union["ShapeDecl", "ShapeOr", "ShapeAnd", "ShapeNot", NodeConstraint, "Shape", typing.Union[IRIREF, BNODE], ShapeExternal], 1, None)]}
    _strict = True

    def __init__(self,
                 imports: typing.Optional[typing.List[str]] = None,
                 startActs: typing.Optional[typing.List[SemAct]] = None,
                 start: typing.Optional[typing.Union["ShapeDecl", "ShapeOr", "ShapeAnd", "ShapeNot", NodeConstraint, "Shape", typing.Union[str, str], ShapeExternal]] = None,
                 shapes: typing.Optional[typing.List[typing.Union["ShapeDecl", "ShapeOr", "ShapeAnd", "ShapeNot", NodeConstraint, "Shape", typing.Union[str, str], ShapeExternal]]] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        setattr(self, '@context', _kwargs.get('@context', None))
        self.imports = imports
        self.startActs = startActs
        self.start = start
        self.shapes = shapes



class ShapeDecl(jsg.JSGObject):
    _reference_types = []
    _members = {'id': typing.Optional[shapeExprLabel],
                'abstract': typing.Optional[jsg.Boolean],
                'restricts': typing.Optional[jsg.ArrayFactory('restricts', _CONTEXT, typing.Union[IRIREF, BNODE], 1, None)],
                'shapeExpr': "shapeExpr"}
    _strict = True

    def __init__(self,
                 id: typing.Optional[typing.Union[str, str]] = None,
                 abstract: typing.Optional[bool] = None,
                 restricts: typing.Optional[typing.List[typing.Union[str, str]]] = None,
                 shapeExpr: typing.Union["ShapeDecl", "ShapeOr", "ShapeAnd", "ShapeNot", NodeConstraint, "Shape", typing.Union[str, str], ShapeExternal] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.id = id
        self.abstract = abstract
        self.restricts = restricts
        self.shapeExpr = shapeExpr



class ShapeOr(jsg.JSGObject):
    _reference_types = []
    _members = {'id': typing.Optional[shapeExprLabel],
                'shapeExprs': jsg.ArrayFactory('shapeExprs', _CONTEXT, typing.Union[ShapeDecl, "ShapeOr", "ShapeAnd", "ShapeNot", NodeConstraint, "Shape", typing.Union[IRIREF, BNODE], ShapeExternal], 2, None)}
    _strict = True

    def __init__(self,
                 id: typing.Optional[typing.Union[str, str]] = None,
                 shapeExprs: typing.List[typing.Union[ShapeDecl, "ShapeOr", "ShapeAnd", "ShapeNot", NodeConstraint, "Shape", typing.Union[str, str], ShapeExternal]] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.id = id
        self.shapeExprs = shapeExprs



class ShapeAnd(jsg.JSGObject):
    _reference_types = []
    _members = {'id': typing.Optional[shapeExprLabel],
                'shapeExprs': jsg.ArrayFactory('shapeExprs', _CONTEXT, typing.Union[ShapeDecl, ShapeOr, "ShapeAnd", "ShapeNot", NodeConstraint, "Shape", typing.Union[IRIREF, BNODE], ShapeExternal], 2, None)}
    _strict = True

    def __init__(self,
                 id: typing.Optional[typing.Union[str, str]] = None,
                 shapeExprs: typing.List[typing.Union[ShapeDecl, ShapeOr, "ShapeAnd", "ShapeNot", NodeConstraint, "Shape", typing.Union[str, str], ShapeExternal]] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.id = id
        self.shapeExprs = shapeExprs



class ShapeNot(jsg.JSGObject):
    _reference_types = []
    _members = {'id': typing.Optional[shapeExprLabel],
                'shapeExpr': "shapeExpr"}
    _strict = True

    def __init__(self,
                 id: typing.Optional[typing.Union[str, str]] = None,
                 shapeExpr: typing.Union[ShapeDecl, ShapeOr, ShapeAnd, "ShapeNot", NodeConstraint, "Shape", typing.Union[str, str], ShapeExternal] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.id = id
        self.shapeExpr = shapeExpr



class Shape(jsg.JSGObject):
    _reference_types = []
    _members = {'id': typing.Optional[shapeExprLabel],
                'extends': typing.Optional[jsg.ArrayFactory('extends', _CONTEXT, typing.Union[IRIREF, BNODE], 1, None)],
                'closed': typing.Optional[jsg.Boolean],
                'extra': typing.Optional[jsg.ArrayFactory('extra', _CONTEXT, IRIREF, 1, None)],
                'expression': typing.Optional["tripleExpr"],
                'semActs': typing.Optional[jsg.ArrayFactory('semActs', _CONTEXT, SemAct, 1, None)],
                'annotations': typing.Optional[jsg.ArrayFactory('annotations', _CONTEXT, Annotation, 1, None)]}
    _strict = True

    def __init__(self,
                 id: typing.Optional[typing.Union[str, str]] = None,
                 extends: typing.Optional[typing.List[typing.Union[str, str]]] = None,
                 closed: typing.Optional[bool] = None,
                 extra: typing.Optional[typing.List[str]] = None,
                 expression: typing.Optional[typing.Union["EachOf", "OneOf", "TripleConstraint", typing.Union[str, str]]] = None,
                 semActs: typing.Optional[typing.List[SemAct]] = None,
                 annotations: typing.Optional[typing.List[Annotation]] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.id = id
        self.extends = extends
        self.closed = closed
        self.extra = extra
        self.expression = expression
        self.semActs = semActs
        self.annotations = annotations



tripleExpr = typing.Union["EachOf", "OneOf", "TripleConstraint", typing.Union[IRIREF, BNODE]]


class EachOf(jsg.JSGObject):
    _reference_types = []
    _members = {'id': typing.Optional[tripleExprLabel],
                'expressions': jsg.ArrayFactory('expressions', _CONTEXT, typing.Union["EachOf", "OneOf", "TripleConstraint", typing.Union[IRIREF, BNODE]], 2, None),
                'min': typing.Optional[jsg.Integer],
                'max': typing.Optional[jsg.Integer],
                'semActs': typing.Optional[jsg.ArrayFactory('semActs', _CONTEXT, SemAct, 1, None)],
                'annotations': typing.Optional[jsg.ArrayFactory('annotations', _CONTEXT, Annotation, 1, None)]}
    _strict = True

    def __init__(self,
                 id: typing.Optional[typing.Union[str, str]] = None,
                 expressions: typing.List[typing.Union["EachOf", "OneOf", "TripleConstraint", typing.Union[str, str]]] = None,
                 min: typing.Optional[int] = None,
                 max: typing.Optional[int] = None,
                 semActs: typing.Optional[typing.List[SemAct]] = None,
                 annotations: typing.Optional[typing.List[Annotation]] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.id = id
        self.expressions = expressions
        self.min = min
        self.max = max
        self.semActs = semActs
        self.annotations = annotations



class OneOf(jsg.JSGObject):
    _reference_types = []
    _members = {'id': typing.Optional[tripleExprLabel],
                'expressions': jsg.ArrayFactory('expressions', _CONTEXT, typing.Union[EachOf, "OneOf", "TripleConstraint", typing.Union[IRIREF, BNODE]], 2, None),
                'min': typing.Optional[jsg.Integer],
                'max': typing.Optional[jsg.Integer],
                'semActs': typing.Optional[jsg.ArrayFactory('semActs', _CONTEXT, SemAct, 1, None)],
                'annotations': typing.Optional[jsg.ArrayFactory('annotations', _CONTEXT, Annotation, 1, None)]}
    _strict = True

    def __init__(self,
                 id: typing.Optional[typing.Union[str, str]] = None,
                 expressions: typing.List[typing.Union[EachOf, "OneOf", "TripleConstraint", typing.Union[str, str]]] = None,
                 min: typing.Optional[int] = None,
                 max: typing.Optional[int] = None,
                 semActs: typing.Optional[typing.List[SemAct]] = None,
                 annotations: typing.Optional[typing.List[Annotation]] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.id = id
        self.expressions = expressions
        self.min = min
        self.max = max
        self.semActs = semActs
        self.annotations = annotations



class TripleConstraint(jsg.JSGObject):
    _reference_types = []
    _members = {'id': typing.Optional[tripleExprLabel],
                'inverse': typing.Optional[jsg.Boolean],
                'predicate': IRIREF,
                'valueExpr': typing.Optional["shapeExpr"],
                'min': typing.Optional[jsg.Integer],
                'max': typing.Optional[jsg.Integer],
                'onShapeExpression': typing.Optional["shapeExpr"],
                'semActs': typing.Optional[jsg.ArrayFactory('semActs', _CONTEXT, SemAct, 1, None)],
                'annotations': typing.Optional[jsg.ArrayFactory('annotations', _CONTEXT, Annotation, 1, None)]}
    _strict = True

    def __init__(self,
                 id: typing.Optional[typing.Union[str, str]] = None,
                 inverse: typing.Optional[bool] = None,
                 predicate: str = None,
                 valueExpr: typing.Optional[typing.Union[ShapeDecl, ShapeOr, ShapeAnd, ShapeNot, NodeConstraint, Shape, typing.Union[str, str], ShapeExternal]] = None,
                 min: typing.Optional[int] = None,
                 max: typing.Optional[int] = None,
                 onShapeExpression: typing.Optional[typing.Union[ShapeDecl, ShapeOr, ShapeAnd, ShapeNot, NodeConstraint, Shape, typing.Union[str, str], ShapeExternal]] = None,
                 semActs: typing.Optional[typing.List[SemAct]] = None,
                 annotations: typing.Optional[typing.List[Annotation]] = None,
                 **_kwargs: typing.Dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs)
        self.id = id
        self.inverse = inverse
        self.predicate = predicate
        self.valueExpr = valueExpr
        self.min = min
        self.max = max
        self.onShapeExpression = onShapeExpression
        self.semActs = semActs
        self.annotations = annotations



shapeExpr = typing.Union[ShapeDecl, ShapeOr, ShapeAnd, ShapeNot, NodeConstraint, Shape, typing.Union[IRIREF, BNODE], ShapeExternal]

_CONTEXT.NAMESPACE = locals()
