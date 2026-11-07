
class Empty:
    """ An empty element. Serves the same function as None, except that it can be used to indicate that JSGNull
    and AnyType attributes (which both can legitimately have None values) have not been assigned
    """
    def __new__(cls):
        return cls