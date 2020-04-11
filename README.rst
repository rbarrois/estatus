=======
estatus
=======


``estatus`` contains a command line tool and a library to compute the status of
files on a Gentoo system:

.. code-block:: sh

    $ estatus /etc
    orphan /etc/hostname
    modified /etc/hosts
    modified /etc/timezone
    missing /etc/conf.d/net


Features
========

Directory lookup
----------------

``estatus`` supports looking at multiple folders in a run, excluding some:

.. code-block:: sh

    $ estatus /etc /usr /lib --exclude /usr/src


A list of default exclusions is built-in:

.. code-block:: sh

    $ estatus --show-default=exclude
    /dev
    /home
    /proc
    /sys


File types
----------

By default, estatus only looks at regular files:

.. code-block:: sh

    $ estatus --show-default=types
    reg


Additional file types can be added:

.. code-block:: sh

    $ estatus /etc --types=reg,dir,lnk


The names follow the convention from ``stat.st_mode`` (Documented in ``inode(7)``):
``fifo``, ``chr``, ``dir``, ``blk``, ``reg``, ``lnk``, ``sock``.


Working with pipes
------------------

If the list of files is ``-``, ``estatus`` will read files from stdin.

By default, file names will be read as one per line; use ``--null`` to split on ``\0`` instead.

Output can be tuned with the following options:

``--record-sep=X``
    Use ``X`` instead of ``\n`` to split records (can be one or more characters)

``--field-sep=X``
    Use ``X`` instead of `` `` to split fields (can be one or more characters)

``--print0``
    Alias for ``--record-sep='\0'``.
    Takes precedence over ``--record-sep``.


Configuration file
------------------

``estatus`` can read the default value of flags from a TOML configuration file:

.. code-block:: sh

    $ estatus /etc --config=./estatus.toml


.. code-block:: toml

    # estatus.toml
    [options]
    types = ["reg", "dir"]
    exclude = [
        "/proc",
        "/dev",
        "/usr/src",
    ]
    record-sep = ";\n"

Options configured on the command line take precedence.

Every option can be provided from the configuration file, except:

- ``--config``: Includes are not supported;
- ``--show-default``: This debug command doesn't make sense in a configuration file;
- ``<paths>``: The list of paths (or stdin) has to be provided on the command line;
- Option shortcuts:

  - ``--print0``.


CLI style
---------

``estatus`` only supports long options, for readability.

The default value of each value can be accessed through ``estatus --show-default=<option>``:

.. code-block:: sh

    $ estatus --show-default=record-sep
    \n

    # Multi-valued option => one value per line
    $ estatus --show-default=exclude
    /dev
    /home
    /proc
    /sys


The ``--show-default`` helper can be used with a full command line:

.. code-block:: sh

    $ estatus --config ~/.config/estatus/estatusrc --record-sep=';\n' --show-default=record-sep
    ;\n
