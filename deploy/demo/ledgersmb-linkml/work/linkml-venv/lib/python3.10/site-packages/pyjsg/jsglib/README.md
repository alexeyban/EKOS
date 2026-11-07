# JSON Schema Grammar

A simple constraint language (schema) for JSON.

## JSG Definition structure
A JSG Schema definition consists of the following major sections:
1) Processing Directives -- directives that determine which element (if any) identifies the type of a JSON object, exceptions to the type definition rule and the which JSON object names are to be ignored completely.
2) Grammar elements -- object and array constraints
3) String and numeric value type constraints.

Each of these sections is described in more detail below.

### Processing Directives
Processing directives consist of two components, both which are optional:
1) A type directive
2) One or more ignore directives

#### The Type Directive
The `.TYPE` directive identifies a string whose value is the name of the JSON object definition that contains it.  As an example,
```text
.TYPE id ;
```
asserts that all JSON objects will include an "id" string whose value is the object type.  As an example a valid instance of the JSG Schema:
```text
.TYPE id ;

person {name:@string, age:@int}
membership {list_name:@string members:person*}
```

might look like:
```json
{ "id": "membership",
  "list_name": "friends",
  "members": [{"id": "person",
               "name": "John Tweed"},
               {"id": "person",
                "name": "Sally Pope",
                "age": 43}]
 }
```
objects can be omitted from this rule by listing them after the primary `.TYPE`:
```text
.TYPE id - person ;
```
were the `.TYPE` declaration in the previous schema replaced with this, the example would change to:
```json
{ "id": "membership",
  "list_name": "friends",
  "members": [{"name": "John Tweed",
               "age": 17},
              {"name": "Sally Pope",
                "age": 43} ]
 }
```
#### The Ignore Directive
The `.IGNORE` directive identifies a list of object element names that will be ignored during validation.  As an example, 

```text
.TYPE id - person;
.IGNORE height weight location ;

person {name:@string, age:@int}
membership {list_name:@string members:person*}
```
states that the names "height", "weight" and "location" will be ignored during validation.  With this definition the following JSON would be considered valid:
```json
{
  "id" : "membership",
  "list_name": "friends",
  "height": null,
  "members": [{"name": "John Tweed",
               "weight": "unspecified",
               "age": 95},
              {"name": "Sally Pope",
               "location": 12345,
                "age": 43} ]
 }
 ```

### Grammar Elements
A JSG schema can define constraints on any number of JSON objects or arrays.  

#### JSON Object Constraints
A JSG Object Constraint constrains the set of possible name/value pairs that may appear in a given JSON object.  As an example:
```text
.TYPE type - person

company {name: @string, "year founded": @int?, employees: person{2,}}
person {name: @string gender:('m'|'f') "active":@bool details*}
details {id: @string, }
```
defines three object constraints:
* `company` - a JSON object that conforms to the this constraint must have:
     * a "name" element whose value is a JSON string
     * an optional "year founded" element that, if present, must be a JSON number without a decimal point or exponent
     * an "employees" element whose value is a list composed of two or more objects, each which must be valid `person ` instances
* `person` - a conforming JSON object must have:
    * a "name" element of type JSON string
    * a "gender" element of type JSON string whose value must be "m" or "f"
    * an "active" element whose values must be `true` or `false`
* `details` - a conforming JSON object must have exacgtly one "type" element of type JSON string any number of additional elements of any type.

The following sample would pass the above 

```json
{ "type": "company",
  "name": "AB Jones",
  "year founded": 1942,
  "employees": [
    {"name": "Grandon Clifford",
     "gender": "m",
      "active": true
     }, {"name": "Elizabeth Poot",
         "gender": "f",
         "active": false}
  ]}
```

##### Object Constraint Forms
An object constraint can either take one of the following forms:

`<name> {<name/value pair list>[|<name/value pair list>]}`

`<name> {<string constraint> -> <value type>}`

The first form explicitly names the (possibly open ended) list of name/value pairs tha may appear in a conforming JSON object. Examples:
```text
empty_object {}
closed_object {id:.}
open_object {id:@int, }
```
Where:
* `empty_object` - an instance can have no members
* `closed_object` - an instance _must_ have exactly one member, "id" whose value can be any type
 *  `open_object` _must_ have an "id" element that, if present, must be a integral form of a JSON number, but _may_ also have any number of additional elements.

The second form of an object constraint constrains the _form_ of an object name and/or its possible values.  Examples:
```text
uri_map { URI -> @object }
named_uris { . -> URI }
pname_map { PNAME -> URI+ }

@terminals
   ...
```
Where:
* `uri_map` defines an object where the member names must match the URI pattern and whose values must be JSON objects
* `named_uris` defines an object whose values must JSON strings that match the URI pattern 
* `pname_map` defines an object whose names must match the PNAME pattern and whose values must be arrays (lists) containing one or more strings matching the URI pattern

### JSON Array Constraints
A JSON array constraint takes the form: "`[<value type constraint><cardinality>]`".  Examples:
```text
number_list            = [@number]
number_list2 = [@number*]
non_empty_list = [.+]
empty_list               = []
very_small_list          = [.]
empty_or_single_element  = [.?]
obj_array_list = [(@object | @array)]
```
* `number_list` - defines JSON arrays having zero or more values of type JSON number
* `number_list2` - alternate form of number_list
* `non_empty_list` - is a list with at least one value, that can be of any type
* `empty_list` - a list with no members
* `very_small_list` - a list with exactly one member of any type
* `empty_or_single_element` - a list with at most one member of any type
* `obj_array_list` - a list consisting of any number of JSON objects and/or arrays

Note that the surrounding brackets (`[` .. `]`) may be omitted when the cardinality identifies the entity as a list. `[@number*]` and `@number*` are equivalent, as are `[@number+]` and `@number+`, and `[@number{0,}]` and `@number{0,}`.  Note, however, that `[@number?]` defines a list consisting of at most one number while `@number?` defines an optional element of type number.  Similarly, `[@number]` is equivalent to `[@number*]`, while `@number` is equivalent to `@number{1}` (or `@number{1,1}`).


## Value Types
The [JSON] language defines the following `value` types :
* string
* number
* object
* array
* true
* false
* null

JSG can restrict the a value to instances of one or more types:
* `@string`   - value must be a JSON string
* `@number`   - value must be a JSON number
* `@int`      - value type must be a JSON number without a decimal point or exponent
* `@object` or `{@string->.}`   - value must be a JSON object
* `@array` or `[.*]`   - value must be a JSON array
* `@bool` - value must either be `true` or `false`
* `@null` - value must be `null`


### String Constraints
The string representation of JSON `string`, `number` and `bool` can be further constrained through the use of regular expressions.  As an example, `[A-Za-z_][A-Za-z0-9_]*` constrains a JSON value to (strings) that begin with a letter or underscore followed by zero or more letters, undescores and/or numeric digits.

JSG String Constraints must be defined at the end of a JSG Schema document and must be preceeded by the identifier `@terminals`.

A JSG String Constraint name must begin with a capital letter and consist solely of 2 or more capital letters or underscores.  A named string constraint consists of:
 
 1) the string constraint name
 2) a colon (:)
 3) an optional regular expression
 4) an optional type of `@string` (default), `@number`, `@bool` or `@null`
 5) a terminating semicolon (;)

The following example defines seven named string constraints:

1) `STRING_NUM` - a 3 character string representing the numbers between "100" and "999".
2) `HEX` - a hexidecimal digit
3) `UCHAR` - "\u" followed by four hex digits or "\U" followed by eight hex digits
4) `LANG_TAG` - an RDF Literal language tag
5) `TRUE` - a fixed value, `true`
6) `EMPTY` - a mull value

It uses these constraints to define a JSON object constraint, the details of which are dsecribed below.

```
object_def {a:STRING_NUM b:LANG_TAG? c:EMPTY}

@terminals
STRING_NUM       : [1-9][0-9]{2} @string ;
HEX              : [0-9] | [A-F] | [a-f] ;
UCHAR            : '\\u' HEX HEX HEX HEX 
                 | '\\U' HEX HEX HEX HEX HEX HEX HEX HEX ;
HEX              : [0-9] | [A-F] | [a-f] ;
LANG_TAG         : '@' [a-zA-Z] + ('-' [a-zA-Z0-9] +)* ;
TRUE             : 'true' @bool ;
EMPTY            : @null ;
```

### Array Constraints
Array constraints restrict the type and number of elements that can appear in a JSON array. Array constraints can be named or can occur inline in another  object or array definition.  Examples:

```
phone_numbers1 [DIGIT7]         // 
phone_numbers2 [DIGIT7*]
phone_numbers3 [DIGIT7+]
phone_numbers4 [DIGIT7{2}]
phone

@terminals
DIGIT7 : [1][0-9]{2}-[0-9]{4} ;
```

### Object Constraints
JSG can constrain the strings and corresponding values that can appear in a JSON object. JSG can constrain the possible `strings` tha appear in an object, the possible `values` or both.  As an example, the following object constraint:
```
member {name:@string
        age:AGE? 
        'member status':@bool
        details:@array}

@terminals
AGE : 2[1-9] | [3-9][0-9] | 100
```
restricts objects of type `obj1` to those containing a "name" string, the value of which must be a JSON string, an optional "age" string, the value of which must be a positive integer between 21 and 100, a "member status" string the falue of which must be `true` or `false` and a "details" string of type JSON list, the contents of which is unspecified.

A example of valid instance of the `member` object would be:
```json
{ "name": "Grunt P. Snooter",
  "age": 43,
  "member status": true,
  "details": ["blah", 3.14159287, false, null]
 }
```

The following example would fail because of "member" is misspelled
```json
{ "name" : "Grunt P. Snooter",
  "mamber status": true,
  "details": []
}
```
The following example would fail because of an extra type:
```angular2html
{ "name": "Grunt P. Snooter",
  "member status": true,
  "details": ["blah", 3.14159287, false, null],
  "adm date": "Feb 17, 2014"
```




