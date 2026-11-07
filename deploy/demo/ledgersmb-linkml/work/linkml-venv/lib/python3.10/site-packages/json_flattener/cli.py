import os
import io
import sys
import logging
import click
import yaml
import json
from json_flattener import flatten_to_csv, unflatten_from_csv, GlobalConfig, KeyConfig, Serializer


def _get_format(input: str, input_format: str =None, default_format: str = None) -> str:
    if input_format is None:
        if input is None:
            if default_format is not None:
                return default_format
            else:
                raise Exception(f'Must pass input file or default format')
        _, ext = os.path.splitext(input)
        if ext is not None:
            input_format = ext.replace('.', '')
        else:
            if default_format is not None:
                return default_format
            else:
                raise Exception(f'Must pass format option OR use known file suffix: {input}')
    return input_format.lower()

def _is_xsv(fmt: str) -> bool:
    return fmt == 'csv' or fmt == 'tsv'

def _get_config(serializer: str = 'json', serialized_keys = [], multivalued_keys = [], flatten_keys = [],
                config_keys=[]) -> GlobalConfig:
    config = GlobalConfig()
    kcs = config.key_configs
    for k in serialized_keys:
        if k not in kcs:
            kcs[k] = KeyConfig()
        kcs[k].serializers = [Serializer(serializer)]
    for k in multivalued_keys:
        if k not in kcs:
            kcs[k] = KeyConfig()
        kcs[k].is_list = True
    for k in flatten_keys:
        if k not in kcs:
            kcs[k] = KeyConfig()
        kcs[k].flatten = True
    for kv in config_keys:
        [k, v] = kv.split('=')
        if k not in kcs:
            kcs[k] = KeyConfig()
        kc = kcs[k]
        kc.delete = True
        for v in v.split(','):
            if v == 'json':
                kc.serializers = [Serializer('json')]
            elif v == 'yaml':
                kc.serializers = [Serializer('yaml')]
            elif v == 'preserve':
                kc.delete = False
            elif v == 'flat':
                kc.flatten = True
            elif v == 'multivalued':
                kc.is_list = True
                kc.flatten = True
            else:
                raise Exception(f'Unknown config val = {v}')
    return config



FORMATS = ['tsv', 'csv', 'yaml', 'json']

# Click input options common across commands
input_option = click.option(
    "-i",
    "--input",
    required=True,
    type=click.Path(),
    help="Input file, e.g. a SSSOM tsv file.",
)
input_format_option = click.option(
    "-I",
    "--input-format",
    help=f'The string denoting the input format, e.g. {",".join(FORMATS)}',
)
output_option = click.option(
    "-o", "--output", help="Output file, e.g. a SSSOM tsv file."
)
output_format_option = click.option(
    "-t",
    "--output-format",
    help=f'Desired output format, e.g. {",".join(FORMATS)}',
)
output_directory_option = click.option(
    "-d", "--output-directory", type=click.Path(), help="Output directory path."
)
key_option = click.option(
    "-k",
    "--key",
    help="Key in root object to be used.",
)
serializer_option = click.option(
    "-s",
    "--serializer",
    help="Serializer to use for complex keys",
)
serialized_keys_option = click.option(
    "-S",
    "--serialized-keys",
    multiple=True,
    help="List of keys that are to be serialized using the serializer",
)
multivalued_keys_option = click.option(
    "-L",
    "--multivalued-keys",
    multiple=True,
    help="List of keys that are multivalued",
)
flatten_keys_option = click.option(
    "-F",
    "--flatten-keys",
    multiple=True,
    help="List of keys that are to be flattened",
)
config_option = click.option(
    "-C",
    "--config-key",
    multiple=True,
    help="Key configuration. Must be of form KEY={yaml,json,flat,multivalued}*",
)
load_config_option = click.option(
    "-c",
    "--load-config",
    help="Path to global configuration file to be loaded",
)
save_config_option = click.option(
    "-O",
    "--save-config",
    help="Path to global configuration file to be saved",
)
@click.group()
@click.option("-v", "--verbose", count=True)
@click.option("-q", "--quiet")
def main(verbose: int, quiet: bool):
    """Main

    Args:

        verbose (int): Verbose.
        quiet (bool): Quiet.

    Returns:

        None.

    """
    if verbose >= 2:
        logging.basicConfig(level=logging.DEBUG)
    elif verbose == 1:
        logging.basicConfig(level=logging.INFO)
    else:
        logging.basicConfig(level=logging.WARNING)
    if quiet:
        logging.basicConfig(level=logging.ERROR)

@main.command()
@input_option
@input_format_option
@output_option
@output_format_option
@multivalued_keys_option
@flatten_keys_option
@serializer_option
@serialized_keys_option
@config_option
@load_config_option
@save_config_option
@key_option
def flatten(input: str, output: str, input_format: str, output_format: str, key: str,
            serializer: str, serialized_keys = [], multivalued_keys = [], flatten_keys = [],
            save_config: str = None, load_config: str = None,
            config_key = []):
    """Flatten a file to TSV/CSV

    Example:
        jfl flatten --input my.yaml --output my.tsv

    Args:

        input (str): The path to the input YAML or JSON file
        output (str): The path to the output file.
        output_format (str): The format to which the TSV should be converted.
    """
    input_format = _get_format(input, input_format)
    output_format = _get_format(output, output_format)
    with open(input) as stream:
        if input_format == 'yaml':
            obj = yaml.safe_load(stream)
        elif input_format == 'json':
            obj = json.load(stream)
    if isinstance(obj, list):
        objs = obj
    elif key is not None:
        objs = obj[key]
    elif isinstance(obj, dict) and len(obj.keys()) == 1:
        key = list(obj.keys())[0]
        logging.warning(f'Selecting key automatically. Better to be explicit and pass in --key {key}')
        objs = obj[key]
    else:
        objs = [obj]
    logging.debug(f'INPUT={objs}')
    if not isinstance(objs, list):
        raise Exception(f'Obj must be a list')
    config = _get_config(serializer = serializer,
                         serialized_keys = serialized_keys,
                         multivalued_keys = multivalued_keys,
                         flatten_keys = flatten_keys,
                         config_keys = config_key)
    logging.debug(f'CONFIG={config}')
    with open(output, 'w') as stream:
        flatten_to_csv(objs, stream, config=config)
    if save_config is not None:
        with open(save_config , 'w') as stream:
            yaml.dump(config.as_dict(), stream)


@main.command()
@input_option
@input_format_option
@output_option
@output_format_option
@multivalued_keys_option
@flatten_keys_option
@serializer_option
@serialized_keys_option
@config_option
@load_config_option
@key_option
def unflatten(input: str, output: str, input_format: str, output_format: str, key: str,
              serializer: str, serialized_keys = [], multivalued_keys = [], flatten_keys = [],
              load_config: str = None,
              config_key = []):
    """Unflatten a file from TSV/CSV

    Example:
        jfl unflatten --input my.tsv --output my.yaml

    """
    input_format = _get_format(input, input_format)
    output_format = _get_format(output, output_format, 'json')
    config = _get_config(serializer = serializer,
                         serialized_keys = serialized_keys,
                         multivalued_keys = multivalued_keys,
                         flatten_keys = flatten_keys,
                         config_keys = config_key)
    if load_config is not None:
        with open(load_config) as stream:
            config = GlobalConfig.from_dict(**yaml.safe_load(stream))
    logging.debug(f'CONFIG={config}')
    with open(input) as stream:
        if input_format == 'tsv':
            sep = '\t'
        elif input_format == 'csv':
            sep = ','
        else:
            logging.warning(f'Guessing separator: {sep}')
            sep = '\t'
        objs = unflatten_from_csv(stream, config)
    logging.debug(f'INPUT={objs}')
    if key is not None:
        obj = {key: objs}
    else:
        obj = objs
    with open(output, 'w') as stream:
        if output_format == 'yaml':
            yaml.safe_dump(obj, stream=stream)
        else:
            json.dump(obj, stream)


if __name__ == "__main__":
    main()
