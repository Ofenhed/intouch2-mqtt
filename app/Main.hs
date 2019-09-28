{-# LANGUAGE OverloadedStrings #-}

module Main where

import Lib
import NetworkPackage
import NetworkPackageParser

import Text.ParserCombinators.ReadP
import Network.Socket
import Network.Socket.ByteString as BS
import System.Environment (getArgs, getProgName)

import qualified Data.ByteString.Char8 as C8

networkHandler sock key dst = do
  response <- BS.recv sock 4096
  let pkg = readP_to_S parsePackage $ C8.unpack response
  putStrLn $ show pkg
  networkHandler sock key dst
  

main :: IO ()
main = withSocketsDo $ do
  args <- getArgs
  case args of
    [host, port, key] -> do
      let sockHints = defaultHints { addrFlags = [AI_NUMERICHOST, AI_NUMERICSERV], addrSocketType = Datagram }
      addr:_ <- getAddrInfo (Just sockHints) (Just host) (Just port)
      sock <- socket (addrFamily addr) (addrSocketType addr) (addrProtocol addr)
      connect sock (addrAddress addr)
      BS.send sock $ C8.pack $ show $ HelloPackage "1"
      response <- BS.recv sock 4096
      let pkg = readP_to_S parsePackage $ C8.unpack response
      case pkg of
        [([HelloPackage x], "")] -> do
          case C8.elemIndex '|' x of
            Nothing -> putStrLn $ "Unexpected hello from server: " ++ C8.unpack response
            Just i -> do
              let (dst, splitter_and_name) = C8.splitAt i x
              putStrLn $ "Connected to " ++ (C8.unpack $ C8.tail splitter_and_name)
              BS.send sock $ C8.pack $ show $ HelloPackage $ C8.pack key
              BS.send sock $ C8.pack $ show $ AuthorizedNetworkPackage (Just $ C8.pack key) (Just dst) NetGetVersion
              networkHandler sock (C8.pack key) dst
        _ -> putStrLn $ "Unexpected response: " ++ C8.unpack response
    _ -> getProgName >>= \progName -> putStrLn $ "Args: " ++ progName ++ " IP PORT KEY"
