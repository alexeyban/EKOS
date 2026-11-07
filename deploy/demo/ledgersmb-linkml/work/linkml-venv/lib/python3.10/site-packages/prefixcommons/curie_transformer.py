import csv
from .curie_util import expand_uri, contract_uri


class Transformer(object):
    """
    Abstract base class for transformer objects
    """
    def __init__(self, cmaps=None, strict=False, contract=False):
        self.cmaps = cmaps
        self.strict = strict
        self.contract = contract
    
    def tr_element(self, x):
        if self.contract:
            ids = []
            if self.cmaps is None:
                ids = contract_uri(x, strict=self.strict)
            else:
                ids = contract_uri(x, cmaps=self.cmaps, strict=self.strict)
            if len(ids) > 0:
                return ids[0]
            else:
                return x
        else:
            if self.cmaps is None:
                return expand_uri(x, strict=self.strict)
            else:
                return expand_uri(x, cmaps=self.cmaps, strict=self.strict)


class CsvTransformer(Transformer):
    """
    Transformer that operates on CSVs/TSVs, expanding or contracting
    column values
    """

    def __init__(self, delimiter="\t", **args):
        self.delimiter = delimiter
        super().__init__(**args)

    def transform(self, infn, outfn):
        with open(infn) as infile:
            reader = csv.reader(infile, delimiter=self.delimiter)
            with open(outfn, 'w') as outfile:
                writer = csv.writer(outfile, delimiter=self.delimiter)
                for row in reader:
                    row = [self.tr_element(x) for x in row]
                    writer.writerow(row)
